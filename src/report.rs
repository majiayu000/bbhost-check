//! text / json / markdown output.

use std::fmt::Write;
use std::path::Path;

use crate::scan::{Action, Report};

pub const ANNOUNCEMENT: &str = "https://community.atlassian.com/forums/Bitbucket-articles/Upcoming-change-to-Bitbucket-Cloud-SSH-access-move-from/ba-p/3234032";

pub fn text(report: &Report) -> String {
    let mut out = String::new();
    let mut current: Option<&Path> = None;
    for f in &report.findings {
        if current != Some(f.path.as_path()) {
            if current.is_some() {
                out.push('\n');
            }
            let _ = writeln!(out, "{}  ({})", f.path.display(), f.kind.label());
            current = Some(&f.path);
        }
        let n = f.line.to_string();
        let pad = " ".repeat(n.len());
        match (&f.action, &f.new) {
            (Action::Append, Some(new)) => {
                let _ = writeln!(out, "  {n}  has  {}", f.old.trim());
                for l in new.lines() {
                    let _ = writeln!(out, "  {pad}  +    {l}");
                }
            }
            (_, Some(new)) => {
                let _ = writeln!(out, "  {n}  -  {}", f.old.trim());
                let _ = writeln!(out, "  {pad}  +  {}", new.trim());
            }
            (_, None) => {
                let _ = writeln!(out, "  {n}  !  {}", f.old.trim());
            }
        }
        if let Some(note) = f.note {
            let _ = writeln!(out, "  {pad}     {note}");
        }
    }
    if !report.findings.is_empty() {
        out.push('\n');
    }
    out.push_str(&summary(report));
    out.push('\n');
    for note in &report.notes {
        let _ = writeln!(out, "note: {note}");
    }
    out
}

pub fn summary(report: &Report) -> String {
    if report.findings.is_empty() {
        return format!(
            "No SSH use of bitbucket.org found in {} file(s).",
            report.files_scanned
        );
    }
    let fixable = report.fixable();
    let manual = report.findings.len() - fixable;
    let mut s = format!(
        "{} finding(s) in {} file(s), {} file(s) scanned. {fixable} can be rewritten with --fix",
        report.findings.len(),
        report.files_with_findings(),
        report.files_scanned
    );
    if manual > 0 {
        let _ = write!(s, ", {manual} need a manual change");
    }
    s.push('.');
    s
}

pub fn json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("report serializes") + "\n"
}

fn cell(s: &str) -> String {
    format!("`{}`", s.trim().replace('|', "\\|").replace('`', "'"))
}

pub fn markdown(report: &Report) -> String {
    let mut out = String::from("### Bitbucket SSH host migration\n\n");
    let _ = writeln!(
        out,
        "Bitbucket Cloud is moving Git over SSH from `bitbucket.org` to `ssh.bitbucket.org`; SSH to `bitbucket.org` is refused starting 2026-11-12 ([official announcement]({ANNOUNCEMENT})). HTTPS URLs are not affected.\n"
    );
    if report.findings.is_empty() {
        let _ = writeln!(
            out,
            "No SSH use of `bitbucket.org` found ({} files scanned).",
            report.files_scanned
        );
        return out;
    }
    out.push_str("| Where | Kind | Now | Change to |\n|---|---|---|---|\n");
    for f in &report.findings {
        let change = match (&f.action, &f.new) {
            (Action::Append, Some(new)) => format!(
                "append {}",
                new.lines().map(cell).collect::<Vec<_>>().join("<br>")
            ),
            (_, Some(new)) => cell(new),
            (_, None) => format!("manual: {}", f.note.unwrap_or("")),
        };
        let _ = writeln!(
            out,
            "| `{}:{}` | {} | {} | {change} |",
            f.path.display(),
            f.line,
            f.kind.label(),
            cell(&f.old)
        );
    }
    let _ = writeln!(out, "\n{}", summary(report));
    for note in &report.notes {
        let _ = writeln!(out, "\n> {note}");
    }
    out
}
