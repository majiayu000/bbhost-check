//! `~/.ssh/config`: `Host bitbucket.org` blocks and `HostName bitbucket.org` aliases.

use crate::rules::{NEW_HOST, OLD_HOST};

pub struct Hit {
    pub line: usize,
    pub old: String,
    pub new: String,
    pub note: &'static str,
}

#[derive(Default)]
struct Block {
    host_line: Option<usize>,
    patterns: Vec<String>,
    hostname: Option<usize>,
}

/// Splits `Key value` / `Key=value` into the keyword and the byte offset of the value.
fn keyword(content: &str) -> Option<(&str, usize)> {
    let trimmed = content.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let kw_end = trimmed
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(trimmed.len());
    let after = &trimmed[kw_end..];
    let value = after.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    Some((&trimmed[..kw_end], content.len() - value.len()))
}

fn value(content: &str, offset: usize) -> &str {
    content[offset..].trim().trim_matches('"')
}

pub fn scan(lines: &[&str]) -> Vec<Hit> {
    let mut blocks = vec![Block::default()];
    for (idx, content) in lines.iter().enumerate() {
        let Some((kw, offset)) = keyword(content) else {
            continue;
        };
        if kw.eq_ignore_ascii_case("host") {
            blocks.push(Block {
                host_line: Some(idx),
                patterns: value(content, offset)
                    .split_whitespace()
                    .map(str::to_ascii_lowercase)
                    .collect(),
                hostname: None,
            });
        } else if kw.eq_ignore_ascii_case("match") {
            blocks.push(Block::default());
        } else if kw.eq_ignore_ascii_case("hostname") {
            blocks.last_mut().expect("blocks is never empty").hostname = Some(idx);
        }
    }

    let new_host_configured = blocks
        .iter()
        .any(|b| b.patterns.iter().any(|p| p == NEW_HOST));
    let mut hits = Vec::new();
    for block in &blocks {
        if let Some(idx) = block.hostname {
            let content = lines[idx];
            let (_, offset) = keyword(content).expect("hostname line has a keyword");
            if value(content, offset).eq_ignore_ascii_case(OLD_HOST) {
                hits.push(Hit {
                    line: idx + 1,
                    old: content.to_string(),
                    new: format!("{}{NEW_HOST}", &content[..offset]),
                    note: "this alias connects to bitbucket.org over SSH",
                });
            }
        } else if let Some(idx) = block.host_line
            && !new_host_configured
            && block.patterns.iter().any(|p| p == OLD_HOST)
        {
            let content = lines[idx];
            hits.push(Hit {
                line: idx + 1,
                old: content.to_string(),
                new: format!("{} {NEW_HOST}", content.trim_end()),
                note: "settings in this block stop applying once remotes use ssh.bitbucket.org",
            });
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Vec<(usize, String)> {
        let lines: Vec<&str> = text.lines().collect();
        scan(&lines).into_iter().map(|h| (h.line, h.new)).collect()
    }

    #[test]
    fn host_block_gets_new_pattern() {
        let text =
            "Host github.com\n  User git\n\nHost bitbucket.org\n  IdentityFile ~/.ssh/id_bb\n";
        assert_eq!(
            run(text),
            vec![(4, "Host bitbucket.org ssh.bitbucket.org".to_string())]
        );
    }

    #[test]
    fn hostname_alias_is_rewritten() {
        let text = "Host bb-work\n  HostName bitbucket.org\n  IdentityFile ~/.ssh/work\nHost x\n  Hostname=Bitbucket.org\n";
        assert_eq!(
            run(text),
            vec![
                (2, "  HostName ssh.bitbucket.org".to_string()),
                (5, "  Hostname=ssh.bitbucket.org".to_string())
            ]
        );
    }

    #[test]
    fn already_migrated_is_quiet() {
        assert!(run("Host bitbucket.org\n  HostName ssh.bitbucket.org\n").is_empty());
        assert!(
            run("Host bitbucket.org\n  User git\nHost ssh.bitbucket.org\n  User git\n").is_empty()
        );
        assert!(run("Host bitbucket.org ssh.bitbucket.org\n  IdentityFile x\n").is_empty());
        assert!(run("# Host bitbucket.org\nHost *.bitbucket.org\n").is_empty());
    }

    #[test]
    fn match_block_resets_scope() {
        assert!(
            run("Host bitbucket.org\n  HostName ssh.bitbucket.org\nMatch all\n  User git\n")
                .is_empty()
        );
    }
}
