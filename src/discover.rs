//! Finds the files to scan and decides which rules apply to each.

use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::WalkDir;

use crate::scan::Kind;

const SKIP_DIRS: &[&str] = &["node_modules", "target", ".venv", "venv"];

const DEPENDENCY_FILES: &[&str] = &[
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.toml",
    "Cargo.lock",
    "go.mod",
    "go.sum",
    "Gemfile",
    "Gemfile.lock",
    "pyproject.toml",
    "poetry.lock",
    "uv.lock",
];

pub struct Target {
    pub path: PathBuf,
    pub kind: Kind,
}

/// Files outside any repository: user and system git config, ssh config, known_hosts.
pub fn machine_targets() -> Result<Vec<Target>, String> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or("cannot find the home directory (HOME is unset); pass --no-machine")?;
    let xdg = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from);

    let mut targets = vec![
        Target {
            path: home.join(".gitconfig"),
            kind: Kind::GlobalGitConfig,
        },
        Target {
            path: xdg.join("git").join("config"),
            kind: Kind::GlobalGitConfig,
        },
        Target {
            path: home.join(".ssh").join("config"),
            kind: Kind::SshConfig,
        },
        Target {
            path: home.join(".ssh").join("known_hosts"),
            kind: Kind::KnownHosts,
        },
    ];
    targets.extend(system_gitconfigs().into_iter().map(|path| Target {
        path,
        kind: Kind::SystemGitConfig,
    }));
    targets.retain(|t| t.path.is_file());
    Ok(targets)
}

fn system_gitconfigs() -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    if env::var_os("GIT_CONFIG_NOSYSTEM").is_some_and(|v| !v.is_empty() && v != "0" && v != "false")
    {
        return paths;
    }
    if let Some(p) = env::var_os("GIT_CONFIG_SYSTEM") {
        paths.insert(PathBuf::from(p));
        return paths;
    }
    // git knows its own prefix (Homebrew, Xcode, Git for Windows); only files with entries show up.
    if let Ok(out) = Command::new("git")
        .args(["config", "--system", "--list", "--show-origin"])
        .output()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some((origin, _)) = line.split_once('\t')
                && let Some(file) = origin.strip_prefix("file:")
            {
                paths.insert(PathBuf::from(file));
            }
        }
    }
    paths.insert(PathBuf::from("/etc/gitconfig"));
    paths
}

/// Walks `root`; unreadable directories are reported through `warn` and skipped.
pub fn root_targets(root: &Path, warn: &mut dyn FnMut(String)) -> Vec<Target> {
    let mut out = Vec::new();
    let mut it = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = it.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn(format!(
                    "cannot read {}: {e}",
                    e.path().unwrap_or(root).display()
                ));
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_dir() {
            if entry.depth() > 0 && SKIP_DIRS.contains(&name.as_ref()) {
                it.skip_current_dir();
            } else if name == ".git" {
                git_dir_targets(path, &mut out);
                it.skip_current_dir();
            } else if name.ends_with(".git") && is_bare_repo(path) {
                out.push(Target {
                    path: path.join("config"),
                    kind: Kind::RepoGitConfig,
                });
                it.skip_current_dir();
            }
        } else if entry.file_type().is_file()
            && let Some(kind) = classify(path)
        {
            out.push(Target {
                path: path.to_path_buf(),
                kind,
            });
        }
    }
    out
}

fn is_bare_repo(dir: &Path) -> bool {
    dir.join("HEAD").is_file() && dir.join("config").is_file() && dir.join("objects").is_dir()
}

/// `.git/config` plus each submodule's own config under `.git/modules`.
fn git_dir_targets(git_dir: &Path, out: &mut Vec<Target>) {
    let config = git_dir.join("config");
    if config.is_file() {
        out.push(Target {
            path: config,
            kind: Kind::RepoGitConfig,
        });
    }
    let modules = git_dir.join("modules");
    for entry in WalkDir::new(&modules)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let p = entry.path();
        if entry.file_type().is_file()
            && entry.file_name() == "config"
            && p.parent().is_some_and(|d| d.join("HEAD").is_file())
        {
            out.push(Target {
                path: p.to_path_buf(),
                kind: Kind::RepoGitConfig,
            });
        }
    }
}

fn classify(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_str()?;
    if name.contains(".bbhost-check-") {
        // our own backups (`<file>.bbhost-check-<time>.bak`)
        return None;
    }
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str());
    let grandparent = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|n| n.to_str());
    if name == ".gitmodules" {
        Some(Kind::GitModules)
    } else if name == "bitbucket-pipelines.yml"
        || name == ".gitlab-ci.yml"
        || name.starts_with("Jenkinsfile")
        || ((name.ends_with(".yml") || name.ends_with(".yaml"))
            && parent == Some("workflows")
            && grandparent == Some(".github"))
    {
        Some(Kind::Ci)
    } else if DEPENDENCY_FILES.contains(&name)
        || (name.starts_with("requirements") && name.ends_with(".txt"))
    {
        Some(Kind::Dependency)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_name() {
        assert_eq!(
            classify(Path::new("a/.github/workflows/ci.yaml")),
            Some(Kind::Ci)
        );
        assert_eq!(classify(Path::new("a/workflows/ci.yaml")), None);
        assert_eq!(classify(Path::new("a/Jenkinsfile.release")), Some(Kind::Ci));
        assert_eq!(
            classify(Path::new("a/requirements-dev.txt")),
            Some(Kind::Dependency)
        );
        assert_eq!(classify(Path::new("a/uv.lock")), Some(Kind::Dependency));
        assert_eq!(classify(Path::new("a/.gitmodules")), Some(Kind::GitModules));
        assert_eq!(classify(Path::new("a/README.md")), None);
    }
}
