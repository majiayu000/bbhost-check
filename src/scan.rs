//! Runs the rules over each target and collects findings.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::discover::Target;
use crate::{knownhosts, rules, sshconfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    RepoGitConfig,
    GitModules,
    GlobalGitConfig,
    SystemGitConfig,
    SshConfig,
    KnownHosts,
    Ci,
    Dependency,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::RepoGitConfig => "repo git config",
            Kind::GitModules => ".gitmodules",
            Kind::GlobalGitConfig => "global git config",
            Kind::SystemGitConfig => "system git config",
            Kind::SshConfig => "ssh config",
            Kind::KnownHosts => "known_hosts",
            Kind::Ci => "CI config",
            Kind::Dependency => "dependency",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// Replace line `line` (`old`) with `new`.
    Replace,
    /// Append `new` to the end of the file.
    Append,
    /// Needs a manual change; `new` is `None`.
    Manual,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub kind: Kind,
    pub path: PathBuf,
    pub line: usize,
    pub old: String,
    pub new: Option<String>,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
}

#[derive(Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub files_scanned: usize,
    /// Hints that do not change the findings.
    pub notes: Vec<String>,
    /// Files or directories that could not be checked.
    pub warnings: Vec<String>,
}

impl Report {
    pub fn fixable(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.action != Action::Manual)
            .count()
    }

    pub fn files_with_findings(&self) -> usize {
        let mut paths: Vec<&Path> = self.findings.iter().map(|f| f.path.as_path()).collect();
        paths.dedup();
        paths.len()
    }
}

pub const INSTEADOF_NOTE: &str = "A git config maps git@bitbucket.org to ssh.bitbucket.org with insteadOf, so remotes listed here already work through git on this machine. CI runners, container builds and package managers that read URLs directly do not use that mapping.";

/// Splits into lines without their line endings; `fix` relies on the same split.
pub fn split_lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n')
        .map(|l| l.trim_end_matches('\n').trim_end_matches('\r'))
        .collect()
}

pub fn read_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
}

pub fn scan_targets(targets: &[Target]) -> Report {
    let mut report = Report::default();
    let mut insteadof = false;
    for target in targets {
        match scan_file(target, &mut insteadof) {
            Ok(mut findings) => {
                report.files_scanned += 1;
                report.findings.append(&mut findings);
            }
            Err(e) => report
                .warnings
                .push(format!("skipped {}: {e}", target.path.display())),
        }
    }
    if insteadof
        && report
            .findings
            .iter()
            .any(|f| matches!(f.kind, Kind::RepoGitConfig | Kind::GitModules))
    {
        report.notes.push(INSTEADOF_NOTE.to_string());
    }
    report
}

fn scan_file(target: &Target, insteadof: &mut bool) -> io::Result<Vec<Finding>> {
    let path = &target.path;
    let finding = |line: usize, old: &str, new: Option<String>, action, note| Finding {
        kind: target.kind,
        path: path.clone(),
        line,
        old: old.to_string(),
        new,
        action,
        note,
    };

    if target.kind == Kind::KnownHosts {
        return Ok(knownhosts::scan(path)?
            .map(|h| {
                finding(
                    h.line,
                    &h.old,
                    Some(h.append),
                    Action::Append,
                    Some("no entry for ssh.bitbucket.org; Bitbucket kept the same host keys"),
                )
            })
            .into_iter()
            .collect());
    }

    let text = read_text(path)?;
    let lines = split_lines(&text);
    if target.kind == Kind::SshConfig {
        return Ok(sshconfig::scan(&lines)
            .into_iter()
            .map(|h| finding(h.line, &h.old, Some(h.new), Action::Replace, Some(h.note)))
            .collect());
    }

    let is_gitconfig = matches!(
        target.kind,
        Kind::RepoGitConfig | Kind::GlobalGitConfig | Kind::SystemGitConfig
    );
    let is_package_json = path.file_name().is_some_and(|n| n == "package.json");
    let mut section_url = "";
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let new = if is_gitconfig {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') {
                section_url = trimmed;
            } else if rules::is_insteadof_to_new_host(section_url, line) {
                *insteadof = true;
            }
            rules::rewrite_gitconfig_line(line)
        } else if target.kind == Kind::Ci {
            rules::rewrite_ci_line(line)
        } else {
            rules::rewrite_ssh_urls(line)
        };
        if let Some(new) = new {
            out.push(finding(idx + 1, line, Some(new), Action::Replace, None));
        } else if is_package_json && rules::has_npm_bitbucket_shorthand(line) {
            out.push(finding(
                idx + 1,
                line,
                None,
                Action::Manual,
                Some("npm expands bitbucket: to git@bitbucket.org; use git+ssh://git@ssh.bitbucket.org/<workspace>/<repo>.git#<ref> and reinstall"),
            ));
        }
    }
    Ok(out)
}
