//! Git Credential Manager: from 2026-11-30 Bitbucket blocks OAuth Git requests made by
//! GCM before 3.0.0 (Bitbucket Cloud changelog, 2026-10-13). Read-only: runs
//! `git config` and the helper's `--version`, never `get`/`store`.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use crate::rules::OLD_HOST;
use crate::scan::{Action, Finding, Kind};

const MIN_VERSION: (u64, u64, u64) = (3, 0, 0);

#[cfg(windows)]
const UPGRADE_NOTE: &str = "Bitbucket blocks OAuth Git requests from GCM before 3.0.0 starting 2026-11-30. Git for Windows 2.56.0.windows.3 still bundles GCM 2.9.x (git-for-windows/git#6464 was open on 2026-10-25), so install GCM 3.0.1+ on its own: gcmuser-win-x64-<version>.exe (no admin) or gcm-win-x64-<version>.exe from https://github.com/git-ecosystem/git-credential-manager/releases. Not affected if Bitbucket is signed in with an app password or API token instead of OAuth";
#[cfg(target_os = "macos")]
const UPGRADE_NOTE: &str = "Bitbucket blocks OAuth Git requests from GCM before 3.0.0 starting 2026-11-30. Upgrade with `brew upgrade --cask git-credential-manager`, or install gcm-osx-<arch>-<version>.pkg from https://github.com/git-ecosystem/git-credential-manager/releases. Not affected if Bitbucket is signed in with an app password or API token instead of OAuth";
#[cfg(not(any(windows, target_os = "macos")))]
const UPGRADE_NOTE: &str = "Bitbucket blocks OAuth Git requests from GCM before 3.0.0 starting 2026-11-30. Upgrade with `dotnet tool update -g git-credential-manager`, or install the .deb / tarball (3.0.1+) from https://github.com/git-ecosystem/git-credential-manager/releases. Not affected if Bitbucket is signed in with an app password or API token instead of OAuth";

#[derive(Clone, Debug, PartialEq)]
pub struct Helper {
    pub origin: String,
    pub value: String,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// No GCM in the helper list, GCM not installed, no HTTPS use of Bitbucket, or OAuth disabled.
    NotApplicable,
    UpToDate,
    Outdated {
        helper: Helper,
        version: String,
    },
    Unknown(String),
}

/// Host of a `credential.<url>.helper` key, if the key is URL-scoped.
fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split('/').next()?;
    let host = authority.rsplit('@').next()?;
    Some(host.split(':').next()?.to_ascii_lowercase())
}

fn applies_to_bitbucket(key: &str) -> bool {
    if key.eq_ignore_ascii_case("credential.helper") {
        return true;
    }
    key.strip_prefix("credential.")
        .and_then(|k| k.strip_suffix(".helper"))
        .and_then(url_host)
        .is_some_and(|h| h == OLD_HOST)
}

/// Builds the helper list git uses for https://bitbucket.org from
/// `git config --show-origin --get-regexp '^credential\..*helper$'`:
/// entries in config order, an empty value clears the list so far.
pub fn effective_helpers(out: &str) -> Vec<Helper> {
    let mut helpers = Vec::new();
    for line in out.lines() {
        let Some((origin, rest)) = line.split_once('\t') else {
            continue;
        };
        let (key, value) = rest.split_once(' ').unwrap_or((rest, ""));
        if !applies_to_bitbucket(key) {
            continue;
        }
        if value.trim().is_empty() {
            helpers.clear();
        } else {
            helpers.push(Helper {
                origin: origin.to_string(),
                value: value.trim().to_string(),
            });
        }
    }
    helpers
}

/// First word of a helper value, honouring quotes and `\ ` escapes.
fn first_word(value: &str) -> String {
    let mut word = String::new();
    let mut quote = None;
    let mut chars = value.trim_start_matches('!').trim_start().chars();
    while let Some(c) = chars.next() {
        match (c, quote) {
            ('\\', _) if chars.clone().next().is_some_and(|n| n == ' ' || n == '"') => {
                word.extend(chars.next());
            }
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (c, None) if c.is_whitespace() => break,
            (c, _) => word.push(c),
        }
    }
    word
}

pub fn is_gcm(value: &str) -> bool {
    let word = first_word(value);
    let base = word
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&word)
        .to_ascii_lowercase();
    let base = base.strip_suffix(".exe").unwrap_or(&base);
    matches!(
        base,
        "manager" | "manager-core" | "git-credential-manager" | "git-credential-manager-core"
    )
}

/// How git itself would start the helper: a path runs as is, a bare name `x` runs `git credential-x`.
fn version_command(value: &str) -> (String, Vec<String>) {
    let word = first_word(value);
    if word.contains(['/', '\\']) {
        (word, vec!["--version".into()])
    } else {
        (
            "git".into(),
            vec![format!("credential-{word}"), "--version".into()],
        )
    }
}

/// First `X.Y.Z` in the output, e.g. `2.9.1+3e7a5b...` or `3.0.1`.
pub fn parse_version(out: &str) -> Option<(u64, u64, u64)> {
    out.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|w| {
            let mut it = w.split('.').map(|n| n.parse::<u64>().ok());
            match (it.next(), it.next(), it.next()) {
                (Some(Some(a)), Some(Some(b)), Some(Some(c))) => Some((a, b, c)),
                _ => None,
            }
        })
}

pub fn evaluate(
    helpers: &[Helper],
    uses_https: bool,
    auth_modes: Option<&str>,
    run_version: &mut dyn FnMut(&str) -> io::Result<String>,
) -> Outcome {
    if !uses_https {
        return Outcome::NotApplicable;
    }
    if auth_modes.is_some_and(|m| !m.split(',').any(|x| x.trim().eq_ignore_ascii_case("oauth"))) {
        return Outcome::NotApplicable;
    }
    let Some(helper) = helpers.iter().find(|h| is_gcm(&h.value)) else {
        return Outcome::NotApplicable;
    };
    let out = match run_version(&helper.value) {
        Ok(out) => out,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Outcome::NotApplicable,
        Err(e) => {
            return Outcome::Unknown(format!(
                "credential helper `{}` ({}) did not report a version: {e}",
                helper.value, helper.origin
            ));
        }
    };
    match parse_version(&out) {
        Some(v) if v >= MIN_VERSION => Outcome::UpToDate,
        Some((a, b, c)) => Outcome::Outdated {
            helper: helper.clone(),
            version: format!("{a}.{b}.{c}"),
        },
        None => Outcome::Unknown(format!(
            "could not read a version from `{} --version`: {}",
            helper.value,
            out.trim()
        )),
    }
}

fn git(args: &[&str]) -> io::Result<Option<String>> {
    let out = Command::new("git").args(args).output()?;
    match out.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned())),
        // no matching key
        Some(1) => Ok(None),
        _ => Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        )),
    }
}

fn run_version(value: &str) -> io::Result<String> {
    let (program, args) = version_command(value);
    let out = Command::new(&program).args(&args).output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let kind = if stderr.contains("is not a git command") {
            io::ErrorKind::NotFound
        } else {
            io::ErrorKind::Other
        };
        return Err(io::Error::new(kind, stderr.trim().to_string()));
    }
    Ok(format!("{}{stderr}", String::from_utf8_lossy(&out.stdout)))
}

/// Where the helper is set, as `(path, line number, line)`.
fn locate(helper: &Helper) -> (PathBuf, usize, String) {
    let path = PathBuf::from(
        helper
            .origin
            .strip_prefix("file:")
            .unwrap_or(&helper.origin)
            .trim_matches('"'),
    );
    let found = fs::read_to_string(&path).ok().and_then(|text| {
        text.lines()
            .enumerate()
            .find(|(_, l)| l.contains(&helper.value) && l.to_ascii_lowercase().contains("helper"))
            .map(|(i, l)| (i + 1, l.trim().to_string()))
    });
    let (line, text) = found.unwrap_or((1, format!("helper = {}", helper.value)));
    (path, line, text)
}

/// Machine-level check; `uses_https` says whether any scanned file reaches bitbucket.org over HTTPS.
pub fn check(uses_https: bool) -> Result<Option<Finding>, String> {
    let helpers = match git(&[
        "config",
        "--show-origin",
        "--get-regexp",
        r"^credential\..*helper$",
    ]) {
        Ok(out) => effective_helpers(&out.unwrap_or_default()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "git config failed while checking the credential helper: {e}"
            ));
        }
    };
    let env_modes = std::env::var("GCM_BITBUCKET_AUTHMODES").ok();
    let auth_modes = match env_modes {
        Some(m) => Some(m),
        None => git(&[
            "config",
            "--get-urlmatch",
            "credential.bitbucketAuthModes",
            "https://bitbucket.org",
        ])
        .map_err(|e| {
            format!("git config failed while checking credential.bitbucketAuthModes: {e}")
        })?,
    };
    match evaluate(
        &helpers,
        uses_https,
        auth_modes.as_deref().map(str::trim),
        &mut run_version,
    ) {
        Outcome::Outdated { helper, version } => {
            let (path, line, text) = locate(&helper);
            Ok(Some(Finding {
                kind: Kind::CredentialHelper,
                path,
                line,
                old: format!("{text}  (GCM {version})"),
                new: None,
                action: Action::Manual,
                note: Some(UPGRADE_NOTE),
            }))
        }
        Outcome::Unknown(w) => Err(w),
        Outcome::NotApplicable | Outcome::UpToDate => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLOBAL: &str = "file:/home/me/.gitconfig";

    fn helpers(values: &[&str]) -> Vec<Helper> {
        values
            .iter()
            .map(|v| Helper {
                origin: GLOBAL.into(),
                value: (*v).into(),
            })
            .collect()
    }

    fn run(hs: &[Helper], https: bool, modes: Option<&str>, version: io::Result<&str>) -> Outcome {
        let result = version.map(str::to_string);
        let mut fake = |_: &str| match &result {
            Ok(s) => Ok(s.clone()),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        };
        evaluate(hs, https, modes, &mut fake)
    }

    #[test]
    fn outdated_2_9() {
        let hs = helpers(&["manager"]);
        assert_eq!(
            run(
                &hs,
                true,
                None,
                Ok("2.9.1+5dd8c7e86a0b3f0d7d4c8ab5ec8d5bb2b5ae8b3e\n")
            ),
            Outcome::Outdated {
                helper: hs[0].clone(),
                version: "2.9.1".into()
            }
        );
    }

    #[test]
    fn three_zero_and_later_are_fine() {
        let hs = helpers(&["/usr/local/share/gcm-core/git-credential-manager"]);
        assert_eq!(
            run(&hs, true, None, Ok("3.0.0+1a2b3c4d5e\n")),
            Outcome::UpToDate
        );
        assert_eq!(run(&hs, true, None, Ok("3.0.1\n")), Outcome::UpToDate);
    }

    #[test]
    fn not_installed() {
        let hs = helpers(&["manager"]);
        let missing = Err(io::Error::new(
            io::ErrorKind::NotFound,
            "git: 'credential-manager' is not a git command.",
        ));
        assert_eq!(run(&hs, true, None, missing), Outcome::NotApplicable);
        assert_eq!(run(&[], true, None, Ok("2.9.1")), Outcome::NotApplicable);
    }

    #[test]
    fn non_gcm_helper() {
        let hs = helpers(&[
            "osxkeychain",
            "store --file ~/.git-credentials",
            "!/opt/homebrew/bin/gh auth git-credential",
        ]);
        assert_eq!(run(&hs, true, None, Ok("2.9.1")), Outcome::NotApplicable);
    }

    #[test]
    fn no_https_or_basic_only_is_quiet() {
        let hs = helpers(&["manager"]);
        assert_eq!(run(&hs, false, None, Ok("2.9.1")), Outcome::NotApplicable);
        assert_eq!(
            run(&hs, true, Some("basic"), Ok("2.9.1")),
            Outcome::NotApplicable
        );
        assert!(matches!(
            run(&hs, true, Some("oauth,basic"), Ok("2.9.1")),
            Outcome::Outdated { .. }
        ));
    }

    #[test]
    fn unreadable_version_is_reported() {
        let hs = helpers(&["manager"]);
        assert!(matches!(
            run(&hs, true, None, Ok("Unhandled exception")),
            Outcome::Unknown(_)
        ));
    }

    #[test]
    fn helper_list_follows_git_rules() {
        let out = "file:/etc/gitconfig\tcredential.helper manager\n\
                   file:/home/me/.gitconfig\tcredential.https://github.com.helper \n\
                   file:/home/me/.gitconfig\tcredential.https://github.com.helper !gh auth git-credential\n\
                   file:/home/me/.gitconfig\tcredential.https://bitbucket.org.helper \n\
                   file:/home/me/.gitconfig\tcredential.https://bitbucket.org.helper osxkeychain\n";
        assert_eq!(
            effective_helpers(out),
            vec![Helper {
                origin: GLOBAL.into(),
                value: "osxkeychain".into()
            }]
        );
        let out = "file:C:/Program Files/Git/etc/gitconfig\tcredential.helper manager\n";
        assert_eq!(effective_helpers(out)[0].value, "manager");
    }

    #[test]
    fn helper_names_and_commands() {
        assert!(is_gcm("manager-core"));
        assert!(is_gcm(
            r"C:/Program\ Files/Git/mingw64/bin/git-credential-manager.exe"
        ));
        assert!(is_gcm(
            r#""C:\Program Files\Git\mingw64\bin\git-credential-manager.exe""#
        ));
        assert!(is_gcm("/home/me/.dotnet/tools/git-credential-manager"));
        assert!(!is_gcm("osxkeychain"));
        assert_eq!(
            version_command("manager").1,
            vec!["credential-manager", "--version"]
        );
        assert_eq!(
            version_command(r"C:/Program\ Files/Git/mingw64/bin/git-credential-manager.exe").0,
            "C:/Program Files/Git/mingw64/bin/git-credential-manager.exe"
        );
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("2.0.8-beta+e1f8492d04"), Some((2, 0, 8)));
        assert_eq!(
            parse_version("Git Credential Manager for Windows version 1.20.0"),
            Some((1, 20, 0))
        );
        assert_eq!(parse_version("3.0.1"), Some((3, 0, 1)));
        assert_eq!(parse_version("no version"), None);
    }
}
