use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

/// Copies tests/fixtures into a temp dir, turning `dot-x` into `.x`
/// (a real `.git` directory cannot be committed).
fn sandbox() -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let name = name
                .strip_prefix("dot-")
                .map_or(name.clone(), |n| format!(".{n}"));
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &to.join(name));
            } else {
                fs::copy(entry.path(), to.join(name)).unwrap();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        dir.path(),
    );
    dir
}

fn run(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bbhost-check"))
        .arg("--root")
        .arg(dir.join("code"))
        .args(args)
        .env("HOME", dir.join("home"))
        .env_remove("XDG_CONFIG_HOME")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn findings(dir: &Path) -> Vec<(String, u64, String)> {
    let out = run(dir, &["--format", "json"], "");
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let path = PathBuf::from(f["path"].as_str().unwrap());
            let rel = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (
                rel,
                f["line"].as_u64().unwrap(),
                f["action"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn scan_finds_every_ssh_use_and_nothing_else() {
    let dir = sandbox();
    let out = run(dir.path(), &["--format", "json"], "");
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = findings(dir.path());
    let has = |p: &str, line: u64| got.iter().any(|(gp, gl, _)| gp == p && *gl == line);

    let expected: &[(&str, u64)] = &[
        ("code/web-app/.git/config", 9),
        ("code/web-app/.git/config", 11),
        ("code/web-app/.git/config", 16),
        ("code/web-app/.git/config", 23),
        ("code/web-app/.gitmodules", 3),
        ("code/web-app/bitbucket-pipelines.yml", 11),
        ("code/web-app/bitbucket-pipelines.yml", 20),
        ("code/web-app/.github/workflows/deploy.yml", 15),
        ("code/web-app/.github/workflows/deploy.yml", 16),
        ("code/web-app/package.json", 6),
        ("code/web-app/package.json", 7),
        ("code/web-app/package-lock.json", 9),
        ("code/web-app/package-lock.json", 14),
        ("code/web-app/yarn.lock", 5),
        ("code/web-app/yarn.lock", 7),
        ("code/web-app/pnpm-lock.yaml", 7),
        ("code/web-app/pnpm-lock.yaml", 8),
        ("code/web-app/pnpm-lock.yaml", 11),
        ("code/web-app/pnpm-lock.yaml", 12),
        ("code/api-service/.git/config", 6),
        ("code/api-service/.git/config", 9),
        (
            "code/api-service/.git/modules/vendor/shared-proto/config",
            7,
        ),
        ("code/api-service/Cargo.toml", 8),
        ("code/api-service/Cargo.lock", 8),
        ("code/api-service/Gemfile", 4),
        ("code/api-service/Gemfile.lock", 2),
        ("code/api-service/pyproject.toml", 6),
        ("code/api-service/uv.lock", 7),
        ("code/api-service/poetry.lock", 12),
        ("code/api-service/requirements.txt", 2),
        ("code/api-service/.gitlab-ci.yml", 8),
        ("code/api-service/Jenkinsfile", 6),
        ("home/.gitconfig", 7),
        ("home/.config/git/config", 3),
        ("home/.ssh/config", 5),
        ("home/.ssh/config", 13),
        ("home/.ssh/known_hosts", 1),
    ];
    for (p, l) in expected {
        assert!(has(p, *l), "missing {p}:{l}; got {got:#?}");
    }
    assert_eq!(
        got.len(),
        expected.len(),
        "unexpected extra findings: {got:#?}"
    );
    assert!(!got.iter().any(|(p, _, _)| p.contains("clean-repo")));
    assert!(
        !got.iter()
            .any(|(p, _, _)| p.ends_with("go.sum") || p.ends_with("go.mod"))
    );

    let manual: Vec<_> = got.iter().filter(|f| f.2 == "manual").collect();
    assert_eq!(manual.len(), 1);
    assert_eq!(manual[0].0, "code/web-app/package.json");
    assert!(got.contains(&("home/.ssh/known_hosts".into(), 1, "append".into())));

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["notes"][0].as_str().unwrap().contains("insteadOf"));
}

#[test]
fn fix_rewrites_only_matched_lines_with_backups() {
    let dir = sandbox();
    let before = findings(dir.path());
    let lockfile = dir.path().join("code/web-app/package-lock.json");
    let original = fs::read_to_string(&lockfile).unwrap();

    let out = run(dir.path(), &["--fix", "--yes"], "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // only the npm `bitbucket:` shorthand is left for a human
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    let after = findings(dir.path());
    assert_eq!(after.len(), 1, "{after:#?}");
    assert_eq!(after[0].2, "manual");

    let updated = fs::read_to_string(&lockfile).unwrap();
    let changed: Vec<u64> = original
        .lines()
        .zip(updated.lines())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i as u64 + 1)
        .collect();
    let expected: Vec<u64> = before
        .iter()
        .filter(|f| f.0 == "code/web-app/package-lock.json")
        .map(|f| f.1)
        .collect();
    assert_eq!(changed, expected);
    assert!(updated.contains("git+ssh://git@ssh.bitbucket.org/acme-demo/ui-kit.git#"));
    assert!(updated.contains("https://registry.npmjs.org/left-pad"));

    let backups: Vec<_> = fs::read_dir(lockfile.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("package-lock.json.bbhost-check-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), original);

    let git = fs::read_to_string(dir.path().join("code/web-app/.git/config")).unwrap();
    assert!(git.contains("url = https://bitbucket.org/acme-demo/web-app.git"));
    assert!(git.contains("url = ssh://git@ssh.bitbucket.org:22/acme-demo/web-app-deploy.git"));

    let ssh = fs::read_to_string(dir.path().join("home/.ssh/config")).unwrap();
    assert!(ssh.contains("Host bitbucket.org ssh.bitbucket.org\n"));
    assert!(ssh.contains("  HostName ssh.bitbucket.org\n"));

    let kh = fs::read_to_string(dir.path().join("home/.ssh/known_hosts")).unwrap();
    assert_eq!(
        kh.lines()
            .filter(|l| l.starts_with("ssh.bitbucket.org "))
            .count(),
        2
    );

    // a second run has nothing left to rewrite
    let again = run(dir.path(), &["--fix", "--yes"], "");
    assert!(String::from_utf8_lossy(&again.stdout).contains("0 can be rewritten"));
}

#[test]
fn fix_without_confirmation_writes_nothing() {
    let dir = sandbox();
    let path = dir.path().join("code/web-app/.git/config");
    let original = fs::read_to_string(&path).unwrap();
    let out = run(dir.path(), &["--fix"], "n\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("+\turl = git@ssh.bitbucket.org:acme-demo/web-app.git")
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing was written"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    // EOF on stdin is not a yes either
    let out = run(dir.path(), &["--fix"], "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn hashed_known_hosts() {
    let dir = sandbox();
    let kh = dir.path().join("home/.ssh/known_hosts");
    let status = Command::new("ssh-keygen")
        .arg("-H")
        .arg("-f")
        .arg(&kh)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(!fs::read_to_string(&kh).unwrap().contains("bitbucket.org"));

    assert!(
        findings(dir.path())
            .iter()
            .any(|f| f.0 == "home/.ssh/known_hosts")
    );
    run(dir.path(), &["--fix", "--yes"], "");
    assert!(
        !findings(dir.path())
            .iter()
            .any(|f| f.0 == "home/.ssh/known_hosts")
    );
}

#[test]
fn markdown_and_clean_tree() {
    let dir = sandbox();
    let md = run(dir.path(), &["--format", "markdown"], "");
    let md = String::from_utf8_lossy(&md.stdout);
    assert!(md.contains("| Where | Kind | Now | Change to |"));
    assert!(md.contains("community.atlassian.com"));

    let out = Command::new(env!("CARGO_BIN_EXE_bbhost-check"))
        .args(["--no-machine", "--root"])
        .arg(dir.path().join("code/clean-repo"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("No SSH use of bitbucket.org"));
}

/// A fake `git-credential-manager` that only answers `--version`.
#[cfg(unix)]
fn fake_gcm(dir: &Path, version: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(format!("gcm-{version}/git-credential-manager"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("#!/bin/sh\necho '{version}+0a1b2c3d4e'\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn gcm_findings(dir: &Path) -> Vec<Value> {
    let out = run(dir, &["--format", "json"], "");
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "credential-helper")
        .cloned()
        .collect()
}

#[cfg(unix)]
#[test]
fn gcm_below_3_is_reported_only_when_used_for_bitbucket_https() {
    let dir = sandbox();
    let gitconfig = dir.path().join("home/.gitconfig");
    let base = fs::read_to_string(&gitconfig).unwrap();
    let set_helper = |helper: &Path| {
        fs::write(
            &gitconfig,
            format!("{base}[credential]\n\thelper = {}\n", helper.display()),
        )
        .unwrap();
    };

    let old = fake_gcm(dir.path(), "2.9.1");
    set_helper(&old);
    let found = gcm_findings(dir.path());
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0]["line"], 10);
    assert!(found[0]["old"].as_str().unwrap().contains("(GCM 2.9.1)"));
    assert_eq!(found[0]["action"], "manual");

    // basic auth only: OAuth blocking does not apply
    fs::write(
        &gitconfig,
        format!(
            "{base}[credential]\n\thelper = {}\n\tbitbucketAuthModes = basic\n",
            old.display()
        ),
    )
    .unwrap();
    assert!(gcm_findings(dir.path()).is_empty());

    set_helper(&fake_gcm(dir.path(), "3.0.1"));
    assert!(gcm_findings(dir.path()).is_empty());

    // no HTTPS use of bitbucket.org anywhere in the scanned tree
    set_helper(&old);
    fs::remove_dir_all(dir.path().join("code")).unwrap();
    fs::create_dir(dir.path().join("code")).unwrap();
    assert!(gcm_findings(dir.path()).is_empty());
}
