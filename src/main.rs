mod discover;
mod fix;
mod gcm;
mod knownhosts;
mod report;
mod rules;
mod scan;
mod sshconfig;

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};

use crate::discover::Target;
use crate::scan::Report;

/// Find Git-over-SSH uses of bitbucket.org and rewrite them to ssh.bitbucket.org.
/// Read-only unless --fix is given.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Directory to scan recursively (repeatable). Default: current directory
    #[arg(long, value_name = "DIR")]
    root: Vec<PathBuf>,

    /// Skip ~/.ssh/config, ~/.ssh/known_hosts, the global/system git config and the GCM version check
    #[arg(long)]
    no_machine: bool,

    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Show the diff, ask for confirmation, back up and rewrite the matched lines
    #[arg(long)]
    fix: bool,

    /// With --fix: do not ask for confirmation
    #[arg(long, requires = "fix")]
    yes: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
    Markdown,
}

fn collect_targets(cli: &Cli) -> Result<(Vec<Target>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let roots = if cli.root.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        cli.root.clone()
    };
    let mut targets = Vec::new();
    for root in &roots {
        if !root.is_dir() {
            return Err(format!("--root {}: not a directory", root.display()));
        }
        targets.extend(discover::root_targets(root, &mut |w| warnings.push(w)));
    }
    if !cli.no_machine {
        targets.extend(discover::machine_targets()?);
    }
    let mut seen = HashSet::new();
    targets.retain(|t| seen.insert(t.path.canonicalize().unwrap_or_else(|_| t.path.clone())));
    Ok((targets, warnings))
}

fn scan(cli: &Cli) -> Result<Report, String> {
    let (targets, mut warnings) = collect_targets(cli)?;
    let mut report = scan::scan_targets(&targets);
    if !cli.no_machine {
        match gcm::check(report.https_bitbucket) {
            Ok(Some(finding)) => report.findings.push(finding),
            Ok(None) => {}
            Err(w) => warnings.push(w),
        }
    }
    warnings.append(&mut report.warnings);
    report.warnings = warnings;
    Ok(report)
}

fn print(report: &Report, format: Format) {
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    let out = match format {
        Format::Text => report::text(report),
        Format::Json => report::json(report),
        Format::Markdown => report::markdown(report),
    };
    print!("{out}");
}

fn exit_code(report: &Report) -> ExitCode {
    if !report.findings.is_empty() {
        ExitCode::from(1)
    } else if !report.warnings.is_empty() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

fn confirm(changes: usize, files: usize) -> Result<bool, String> {
    eprint!(
        "Apply {changes} change(s) to {files} file(s)? Backups are written next to each file. [y/N] "
    );
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn run(cli: &Cli) -> Result<ExitCode, String> {
    let report = scan(cli)?;
    if !cli.fix {
        print(&report, cli.format);
        return Ok(exit_code(&report));
    }

    let fixable = report.fixable();
    if fixable == 0 {
        print(&report, cli.format);
        return Ok(exit_code(&report));
    }
    print!("{}", fix::preview(&report.findings));
    let files = fix::file_count(&report.findings);
    if !cli.yes && !confirm(fixable, files)? {
        eprintln!("Aborted; nothing was written.");
        return Ok(ExitCode::from(1));
    }
    let (written, errors) = fix::apply(&report.findings);
    for w in &written {
        eprintln!(
            "wrote {} (backup: {})",
            w.path.display(),
            w.backup.display()
        );
    }
    for e in &errors {
        eprintln!("error: {e}");
    }

    eprintln!("\nAfter --fix:");
    let after = scan(cli)?;
    print(&after, cli.format);
    if !errors.is_empty() {
        return Ok(ExitCode::from(2));
    }
    Ok(exit_code(&after))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
