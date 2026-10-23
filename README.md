# bbhost-check

**Bitbucket SSH migration check before the 2026-11-12 deadline:** one command finds every place on your machine that still uses `git@bitbucket.org` over SSH (remotes, submodules, `~/.ssh/config`, `known_hosts`, git config, CI files, lockfiles), shows the diff, and rewrites it to `git@ssh.bitbucket.org` after you confirm.

Per the [official Atlassian announcement](https://community.atlassian.com/forums/Bitbucket-articles/Upcoming-change-to-Bitbucket-Cloud-SSH-access-move-from/ba-p/3234032), Bitbucket Cloud is splitting traffic: `bitbucket.org` keeps HTTPS (web, API, `https://` clones) and all Git over SSH moves to `ssh.bitbucket.org`. Starting **2026-11-12**, SSH to `bitbucket.org` is blocked during scheduled brownout windows on working days, growing from 15 minutes to 3 hours, and on **2026-12-10** it is removed for good (per the announcement's October 21, 2026 update). The host keys stay the same. HTTPS URLs are not affected.

`bbhost-check` is read-only unless you pass `--fix`. It only touches local files, needs no Bitbucket token, and never changes `https://` URLs.

## Install

Build from source (Rust 1.88+):

```bash
cargo install --git https://github.com/majiayu000/bbhost-check --locked
```

## Usage

Scan a folder of clones plus your machine config:

```bash
bbhost-check --root ~/code
```

Preview the diff, confirm, and rewrite (each file is backed up first):

```bash
bbhost-check --root ~/code --fix
```

Post the findings in a PR or ticket:

```bash
bbhost-check --root . --no-machine --format markdown
```

| Flag | What it does |
|---|---|
| `--root DIR` | Directory to scan recursively; repeatable. Default `.` |
| `--no-machine` | Skip `~/.ssh/config`, `~/.ssh/known_hosts` and the global/system git config |
| `--format text\|json\|markdown` | Output format, default `text` |
| `--fix` | Print the diff, ask `[y/N]`, back up each file to `<file>.bbhost-check-<unix-time>.bak`, then rewrite only the matched lines |
| `--yes` | With `--fix`: skip the question |

Exit codes: `0` nothing found, `1` findings left (use it as a CI check), `2` the tool could not check something (unreadable file, missing `ssh-keygen`, bad arguments).

## What it scans

| Where | What counts as a hit | `--fix` does |
|---|---|---|
| `.git/config` of every repo under `--root`, including bare repos and `.git/modules/*/config` (submodule clones) | `url`, `pushurl` and submodule `url` using SSH to `bitbucket.org`; `[url "git@bitbucket.org:"]` sections | rewrites the host |
| `.gitmodules` | submodule URLs | rewrites the host |
| `~/.gitconfig`, `~/.config/git/config`, system git config | `[url "..."]` sections pointing at the old SSH host (common for Go private modules) | rewrites the host |
| `~/.ssh/config` | `Host bitbucket.org` blocks (their `IdentityFile` stops applying once remotes say `ssh.bitbucket.org`), `HostName bitbucket.org` aliases | appends `ssh.bitbucket.org` to the `Host` line; changes `HostName` |
| `~/.ssh/known_hosts` (plain or hashed) | has `bitbucket.org`, lacks `ssh.bitbucket.org` | appends `ssh.bitbucket.org` entries using the keys you already trust for `bitbucket.org` |
| `bitbucket-pipelines.yml`, `.github/workflows/*.yml`, `.gitlab-ci.yml`, `Jenkinsfile*` | SSH URLs, `ssh-keyscan bitbucket.org` | rewrites the host |
| `package.json`, `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `Cargo.toml`, `Cargo.lock`, `go.mod`, `go.sum`, `Gemfile`, `Gemfile.lock`, `pyproject.toml`, `poetry.lock`, `uv.lock`, `requirements*.txt` | `git@bitbucket.org:`, `ssh://`, `git+ssh://` dependency URLs; npm `bitbucket:` shorthand | rewrites the host; the npm shorthand is reported for a manual change |

Not a hit: `https://bitbucket.org/...` (with or without a user name), `ssh.bitbucket.org`, `altssh.bitbucket.org`, and `insteadOf = git@bitbucket.org:` lines (that is the two-line workaround, not a problem). `node_modules`, `target`, `.venv` and `venv` are skipped.

If you already set the global `insteadOf` workaround, your remotes keep working on this machine, and `bbhost-check` says so. It still lists them, because CI runners, Docker builds and package managers that read URLs directly don't see your git config.

## Example

From [`examples/demo.sh`](examples/demo.sh), which builds a throwaway `HOME` and fake repos (trimmed, with the throwaway home shown as `/home/me`; full output in [`examples/demo-output.txt`](examples/demo-output.txt)):

```text
$ bbhost-check --root code
code/web-app/.git/config  (repo git config)
  9  -  url = git@bitbucket.org:acme-demo/web-app.git
     +  url = git@ssh.bitbucket.org:acme-demo/web-app.git
  11  -  pushurl = git@bitbucket.org:acme-demo/web-app.git
      +  pushurl = git@ssh.bitbucket.org:acme-demo/web-app.git
  16  -  url = ssh://git@bitbucket.org:22/acme-demo/web-app-deploy.git
      +  url = ssh://git@ssh.bitbucket.org:22/acme-demo/web-app-deploy.git

code/web-app/bitbucket-pipelines.yml  (CI config)
  11  -  - ssh-keyscan -t rsa bitbucket.org >> ~/.ssh/known_hosts
      +  - ssh-keyscan -t rsa ssh.bitbucket.org >> ~/.ssh/known_hosts

code/web-app/package.json  (dependency)
  6  -  "@acme-demo/ui-kit": "git+ssh://git@bitbucket.org/acme-demo/ui-kit.git#v3.2.0",
     +  "@acme-demo/ui-kit": "git+ssh://git@ssh.bitbucket.org/acme-demo/ui-kit.git#v3.2.0",
  7  !  "@acme-demo/analytics": "bitbucket:acme-demo/analytics#v0.9.1",
        npm expands bitbucket: to git@bitbucket.org; use git+ssh://git@ssh.bitbucket.org/<workspace>/<repo>.git#<ref> and reinstall

/home/me/.ssh/config  (ssh config)
  5  -  Host bitbucket.org
     +  Host bitbucket.org ssh.bitbucket.org
        settings in this block stop applying once remotes use ssh.bitbucket.org

/home/me/.ssh/known_hosts  (known_hosts)
  1  has  bitbucket.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA...
     +    ssh.bitbucket.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA...
        no entry for ssh.bitbucket.org; Bitbucket kept the same host keys

35 finding(s) in 24 file(s), 28 file(s) scanned. 34 can be rewritten with --fix, 1 need a manual change.
```

After `--fix` (answering `y`), only the npm shorthand is left and `git remote -v` shows the new host:

```text
origin	git@ssh.bitbucket.org:acme-demo/web-app.git (fetch)
origin	git@ssh.bitbucket.org:acme-demo/web-app.git (push)
deploy	ssh://git@ssh.bitbucket.org:22/acme-demo/web-app-deploy.git (fetch)
```

After rewriting a lockfile, run your package manager's install once so it re-checks the result.

## Notes

- Pipelines' default `known_hosts` now includes `ssh.bitbucket.org` ([BCLOUD-24132](https://jira.atlassian.com/browse/BCLOUD-24132), fixed). Bamboo needs 13.0.1, 12.1.12 or 10.2.24 or later for the new URL ([BAM-26573](https://jira.atlassian.com/browse/BAM-26573)).
- If port 22 is blocked, Atlassian documents an alternate host and port ("Port 22 is blocked on local network"). `bbhost-check` leaves `altssh.bitbucket.org` alone; the announcement doesn't say whether it changes.
- Not covered: ssh config `Include` files, scp URLs without a user name (`bitbucket.org:ws/repo`), Dockerfiles, Terraform and Ansible sources.
- Background and design: [`docs/research.md`](docs/research.md), [`docs/design.md`](docs/design.md).

## License

MIT
