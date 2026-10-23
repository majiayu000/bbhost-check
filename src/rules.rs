//! Line rules shared by every file kind except ssh config and known_hosts.

pub const OLD_HOST: &str = "bitbucket.org";
pub const NEW_HOST: &str = "ssh.bitbucket.org";

/// Rewrites every SSH use of `bitbucket.org` in `line` to `ssh.bitbucket.org`.
/// HTTPS URLs and hosts that merely end in `bitbucket.org` are left alone.
pub fn rewrite_ssh_urls(line: &str) -> Option<String> {
    rewrite_where(line, |i| is_ssh_host_at(line, i))
}

/// Git config: the value of `insteadOf` / `pushInsteadOf` is the pattern being
/// replaced, not where git connects, so only the `[url "..."]` header counts.
pub fn rewrite_gitconfig_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with(';') || is_insteadof(trimmed) {
        return None;
    }
    rewrite_ssh_urls(line)
}

/// `true` for an `insteadOf` line that maps the old SSH host onto the new one.
pub fn is_insteadof_to_new_host(section_url: &str, line: &str) -> bool {
    let trimmed = line.trim_start();
    section_url.contains(NEW_HOST)
        && is_insteadof(trimmed)
        && trimmed
            .split_once('=')
            .is_some_and(|(_, value)| rewrite_ssh_urls(value.trim()).is_some())
}

fn is_insteadof(trimmed: &str) -> bool {
    let key = trimmed
        .split(|c: char| c == '=' || c.is_whitespace())
        .next()
        .unwrap_or("");
    key.eq_ignore_ascii_case("insteadof") || key.eq_ignore_ascii_case("pushinsteadof")
}

/// CI files: SSH URLs plus `ssh-keyscan bitbucket.org`.
pub fn rewrite_ci_line(line: &str) -> Option<String> {
    let urls = rewrite_ssh_urls(line);
    let base = urls.as_deref().unwrap_or(line);
    rewrite_keyscan(base).or(urls)
}

fn rewrite_keyscan(line: &str) -> Option<String> {
    let start = line.find("ssh-keyscan")?;
    rewrite_where(line, |i| {
        i > start
            && line[..i].ends_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            && line[i + OLD_HOST.len()..]
                .chars()
                .next()
                .is_none_or(|c| !is_host_char(c))
    })
}

/// npm expands `bitbucket:ws/repo` to `git@bitbucket.org` for SSH. Report only.
pub fn has_npm_bitbucket_shorthand(line: &str) -> bool {
    line.contains("\"bitbucket:")
}

fn rewrite_where(line: &str, mut matches: impl FnMut(usize) -> bool) -> Option<String> {
    let mut out = String::with_capacity(line.len() + 8);
    let mut last = 0;
    let mut hit = false;
    for (i, _) in line.match_indices(OLD_HOST) {
        if matches(i) {
            out.push_str(&line[last..i]);
            out.push_str("ssh.");
            last = i;
            hit = true;
        }
    }
    if !hit {
        return None;
    }
    out.push_str(&line[last..]);
    Some(out)
}

fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')
}

fn is_ssh_scheme(scheme: &str) -> bool {
    // yarn.lock keys look like `pkg@git+ssh://...`; the scheme is after the last `@`.
    let scheme = scheme.rsplit('@').next().unwrap_or(scheme);
    ["ssh", "git+ssh", "ssh+git"]
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
}

fn is_ssh_host_at(line: &str, i: usize) -> bool {
    let before = &line[..i];
    let next = line[i + OLD_HOST.len()..].chars().next();
    let start = before
        .char_indices()
        .rev()
        .find(|&(_, c)| c.is_whitespace() || "\"'`=<>()[]{},".contains(c))
        .map_or(0, |(p, c)| p + c.len_utf8());
    let token = &before[start..];

    if let Some(rest) = token.strip_suffix('@') {
        match rest.find("://") {
            Some(p) => is_ssh_scheme(&rest[..p]) && matches!(next, Some(':' | '/')),
            None => {
                !rest.is_empty()
                    && (next == Some(':')
                        || (rest == "git" && next.is_none_or(|c| !is_host_char(c))))
            }
        }
    } else if let Some(rest) = token.strip_suffix("//") {
        rest.strip_suffix(':').is_some_and(is_ssh_scheme) && matches!(next, Some(':' | '/'))
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw(s: &str) -> Option<String> {
        rewrite_ssh_urls(s)
    }

    #[test]
    fn scp_and_url_forms() {
        assert_eq!(
            rw("url = git@bitbucket.org:acme/app.git").unwrap(),
            "url = git@ssh.bitbucket.org:acme/app.git"
        );
        assert_eq!(
            rw("ssh://git@bitbucket.org/acme/app.git").unwrap(),
            "ssh://git@ssh.bitbucket.org/acme/app.git"
        );
        assert_eq!(
            rw("ssh://git@bitbucket.org:22/acme/app.git").unwrap(),
            "ssh://git@ssh.bitbucket.org:22/acme/app.git"
        );
        assert_eq!(
            rw("ssh://bitbucket.org/acme/app.git").unwrap(),
            "ssh://ssh.bitbucket.org/acme/app.git"
        );
        assert_eq!(
            rw(r#""resolved": "git+ssh://git@bitbucket.org/acme/lib.git#abc""#).unwrap(),
            r#""resolved": "git+ssh://git@ssh.bitbucket.org/acme/lib.git#abc""#
        );
        assert_eq!(
            rw(r#""lib@git+ssh://git@bitbucket.org/acme/lib.git#v1":"#).unwrap(),
            r#""lib@git+ssh://git@ssh.bitbucket.org/acme/lib.git#v1":"#
        );
        assert_eq!(
            rw("ssh -T git@bitbucket.org").unwrap(),
            "ssh -T git@ssh.bitbucket.org"
        );
    }

    #[test]
    fn rewrites_every_occurrence() {
        assert_eq!(
            rw("a git@bitbucket.org:x/y b ssh://git@bitbucket.org/x/z").unwrap(),
            "a git@ssh.bitbucket.org:x/y b ssh://git@ssh.bitbucket.org/x/z"
        );
    }

    #[test]
    fn leaves_https_and_other_hosts() {
        for s in [
            "https://bitbucket.org/acme/app.git",
            "https://jdoe@bitbucket.org/acme/app.git",
            "http://bitbucket.org/acme/app.git",
            "git://bitbucket.org/acme/app.git",
            "git@ssh.bitbucket.org:acme/app.git",
            "ssh://git@altssh.bitbucket.org:443/acme/app.git",
            "git@mybitbucket.org:acme/app.git",
            "git@bitbucket.org.example.com:acme/app.git",
            "contact support@bitbucket.org for help",
            "go.sum: bitbucket.org/acme/mod v1.0.0 h1:abc=",
            "@bitbucket.org:x",
        ] {
            assert_eq!(rw(s), None, "{s}");
        }
    }

    #[test]
    fn gitconfig_skips_insteadof_values() {
        assert_eq!(
            rewrite_gitconfig_line("\tinsteadOf = git@bitbucket.org:"),
            None
        );
        assert_eq!(
            rewrite_gitconfig_line("pushInsteadOf=ssh://git@bitbucket.org/"),
            None
        );
        assert_eq!(
            rewrite_gitconfig_line("# url = git@bitbucket.org:a/b"),
            None
        );
        assert_eq!(
            rewrite_gitconfig_line(r#"[url "git@bitbucket.org:"]"#).unwrap(),
            r#"[url "git@ssh.bitbucket.org:"]"#
        );
        assert_eq!(
            rewrite_gitconfig_line("\tpushurl = git@bitbucket.org:a/b.git").unwrap(),
            "\tpushurl = git@ssh.bitbucket.org:a/b.git"
        );
        assert!(is_insteadof_to_new_host(
            "git@ssh.bitbucket.org:",
            "\tinsteadOf = git@bitbucket.org:"
        ));
        assert!(!is_insteadof_to_new_host(
            "git@bitbucket.org:",
            "\tinsteadOf = https://bitbucket.org/"
        ));
    }

    #[test]
    fn keyscan() {
        assert_eq!(
            rewrite_ci_line("  - ssh-keyscan -t rsa bitbucket.org >> ~/.ssh/known_hosts").unwrap(),
            "  - ssh-keyscan -t rsa ssh.bitbucket.org >> ~/.ssh/known_hosts"
        );
        assert_eq!(rewrite_ci_line("ssh-keyscan ssh.bitbucket.org"), None);
        assert_eq!(rewrite_ci_line("curl https://bitbucket.org/site/ssh"), None);
        assert_eq!(
            rewrite_ci_line("ssh-keyscan bitbucket.org && git clone git@bitbucket.org:a/b.git")
                .unwrap(),
            "ssh-keyscan ssh.bitbucket.org && git clone git@ssh.bitbucket.org:a/b.git"
        );
    }

    #[test]
    fn npm_shorthand() {
        assert!(has_npm_bitbucket_shorthand(
            r#""lib": "bitbucket:acme/lib#v1""#
        ));
        assert!(!has_npm_bitbucket_shorthand(
            r#""lib": "git+ssh://git@ssh.bitbucket.org/acme/lib.git""#
        ));
    }
}
