//! known_hosts: the host keys did not change, but entries are matched by name.
//! Lookups go through `ssh-keygen -F` so hashed entries work too.

use std::io;
use std::path::Path;
use std::process::Command;

use crate::rules::{NEW_HOST, OLD_HOST};

pub struct Hit {
    pub line: usize,
    pub old: String,
    /// Lines to append, reusing the keys already trusted for bitbucket.org.
    pub append: String,
}

/// Returns `(line number, entry)` for every entry matching `host`.
fn lookup(path: &Path, host: &str) -> io::Result<Vec<(usize, String)>> {
    let out = Command::new("ssh-keygen")
        .args(["-F", host, "-f"])
        .arg(path)
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    e.kind(),
                    "ssh-keygen not found; the known_hosts check needs OpenSSH",
                )
            } else {
                e
            }
        })?;
    match out.status.code() {
        Some(0) => {}
        Some(1) => return Ok(Vec::new()),
        _ => {
            return Err(io::Error::other(format!(
                "ssh-keygen -F failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(parse_lookup(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_lookup(stdout: &str) -> Vec<(usize, String)> {
    let mut entries = Vec::new();
    let mut line_no = None;
    for l in stdout.lines() {
        if let Some(rest) = l.strip_prefix('#') {
            line_no = rest
                .rsplit_once("line")
                .and_then(|(_, n)| n.trim().parse::<usize>().ok());
        } else if let Some(n) = line_no.take() {
            entries.push((n, l.to_string()));
        }
    }
    entries
}

fn append_lines(entries: &[(usize, String)]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (_, entry) in entries {
        let fields: Vec<&str> = entry.split_whitespace().collect();
        if fields.len() < 3 || fields[0].starts_with('@') {
            continue;
        }
        let line = format!("{NEW_HOST} {} {}", fields[1], fields[2]);
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

pub fn scan(path: &Path) -> io::Result<Option<Hit>> {
    let old = lookup(path, OLD_HOST)?;
    if old.is_empty() || !lookup(path, NEW_HOST)?.is_empty() {
        return Ok(None);
    }
    let append = append_lines(&old);
    if append.is_empty() {
        return Ok(None);
    }
    let (line, entry) = &old[0];
    Ok(Some(Hit {
        line: *line,
        old: entry.clone(),
        append,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_hashed_lookups() {
        let out = "# Host bitbucket.org found: line 3 \nbitbucket.org ssh-ed25519 AAAA1\n# Host bitbucket.org found: line 7 \n|1|salt=|hash= ecdsa-sha2-nistp256 AAAA2\n";
        let entries = parse_lookup(out);
        assert_eq!(entries[0].0, 3);
        assert_eq!(entries[1].0, 7);
        assert_eq!(
            append_lines(&entries),
            "ssh.bitbucket.org ssh-ed25519 AAAA1\nssh.bitbucket.org ecdsa-sha2-nistp256 AAAA2"
        );
    }

    #[test]
    fn skips_markers_and_duplicates() {
        let entries = vec![
            (1, "@revoked bitbucket.org ssh-rsa AAAA0".to_string()),
            (2, "bitbucket.org ssh-rsa AAAA1".to_string()),
            (3, "bitbucket.org,104.192.141.1 ssh-rsa AAAA1".to_string()),
        ];
        assert_eq!(append_lines(&entries), "ssh.bitbucket.org ssh-rsa AAAA1");
    }
}
