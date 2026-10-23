//! `--fix`: preview, back up, and rewrite only the matched lines.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::scan::{Action, Finding, read_text, split_lines};

fn by_file(findings: &[Finding]) -> BTreeMap<&Path, Vec<&Finding>> {
    let mut map: BTreeMap<&Path, Vec<&Finding>> = BTreeMap::new();
    for f in findings.iter().filter(|f| f.action != Action::Manual) {
        map.entry(&f.path).or_default().push(f);
    }
    map
}

pub fn file_count(findings: &[Finding]) -> usize {
    by_file(findings).len()
}

pub fn preview(findings: &[Finding]) -> String {
    let mut out = String::new();
    for (path, fs) in by_file(findings) {
        let _ = writeln!(out, "--- {}\n+++ {}", path.display(), path.display());
        for f in fs {
            let new = f.new.as_deref().unwrap_or_default();
            if f.action == Action::Append {
                let _ = writeln!(out, "@@ end of file @@");
            } else {
                let _ = writeln!(out, "@@ line {} @@\n-{}", f.line, f.old);
            }
            for l in new.lines() {
                let _ = writeln!(out, "+{l}");
            }
        }
    }
    out
}

pub struct Written {
    pub path: PathBuf,
    pub backup: PathBuf,
}

/// Applies every fixable finding. A file whose matched lines changed since the scan
/// is left untouched and reported as an error; other files are still written.
pub fn apply(findings: &[Finding]) -> (Vec<Written>, Vec<String>) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut written = Vec::new();
    let mut errors = Vec::new();
    for (path, fs) in by_file(findings) {
        match apply_file(path, &fs, stamp) {
            Ok(backup) => written.push(Written {
                path: path.to_path_buf(),
                backup,
            }),
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    (written, errors)
}

fn rewrite(text: &str, fs: &[&Finding]) -> Result<String, String> {
    let lines = split_lines(text);
    let mut pieces: Vec<&str> = text.split_inclusive('\n').collect();
    let mut replaced: Vec<String> = Vec::new();
    let mut targets = Vec::new();
    for f in fs.iter().filter(|f| f.action == Action::Replace) {
        let idx = f.line - 1;
        if lines.get(idx) != Some(&f.old.as_str()) {
            return Err(format!(
                "line {} changed since the scan; nothing written",
                f.line
            ));
        }
        let ending = &pieces[idx][lines[idx].len()..];
        replaced.push(format!("{}{ending}", f.new.as_deref().unwrap_or_default()));
        targets.push(idx);
    }
    for (idx, new) in targets.into_iter().zip(&replaced) {
        pieces[idx] = new;
    }
    let mut out = pieces.concat();
    for f in fs.iter().filter(|f| f.action == Action::Append) {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(f.new.as_deref().unwrap_or_default());
        out.push('\n');
    }
    Ok(out)
}

fn apply_file(path: &Path, fs: &[&Finding], stamp: u64) -> Result<PathBuf, String> {
    let text = read_text(path).map_err(|e| e.to_string())?;
    let new = rewrite(&text, fs)?;
    let name = path.file_name().ok_or("not a file")?.to_string_lossy();
    let backup = path.with_file_name(format!("{name}.bbhost-check-{stamp}.bak"));
    if backup.exists() {
        return Err(format!(
            "backup {} already exists; nothing written",
            backup.display()
        ));
    }
    fs::copy(path, &backup).map_err(|e| format!("backup failed: {e}"))?;
    let tmp = path.with_file_name(format!(".{name}.bbhost-check.tmp"));
    let perms = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    fs::write(&tmp, new).map_err(|e| format!("write failed: {e}"))?;
    fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| format!("rename failed: {e}"))?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::Kind;

    fn finding(line: usize, old: &str, new: &str, action: Action) -> Finding {
        Finding {
            kind: Kind::Dependency,
            path: PathBuf::from("x"),
            line,
            old: old.into(),
            new: Some(new.into()),
            action,
            note: None,
        }
    }

    #[test]
    fn replaces_only_matched_lines_and_keeps_crlf() {
        let text = "a\r\ngit@bitbucket.org:x\r\nb\ngit@bitbucket.org:x";
        let f1 = finding(
            2,
            "git@bitbucket.org:x",
            "git@ssh.bitbucket.org:x",
            Action::Replace,
        );
        let f2 = finding(
            4,
            "git@bitbucket.org:x",
            "git@ssh.bitbucket.org:x",
            Action::Replace,
        );
        assert_eq!(
            rewrite(text, &[&f1, &f2]).unwrap(),
            "a\r\ngit@ssh.bitbucket.org:x\r\nb\ngit@ssh.bitbucket.org:x"
        );
    }

    #[test]
    fn refuses_when_line_changed() {
        let f = finding(1, "old", "new", Action::Replace);
        assert!(rewrite("different\n", &[&f]).is_err());
    }

    #[test]
    fn appends_with_newline() {
        let f = finding(1, "bitbucket.org k", "ssh.bitbucket.org k", Action::Append);
        assert_eq!(
            rewrite("bitbucket.org k", &[&f]).unwrap(),
            "bitbucket.org k\nssh.bitbucket.org k\n"
        );
    }
}
