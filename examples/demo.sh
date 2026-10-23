#!/usr/bin/env bash
# Builds a throwaway HOME plus a few fake repos under $1 and runs bbhost-check there.
# Never touches your real ~/.ssh, git config or repositories.
set -euo pipefail

demo="${1:?usage: examples/demo.sh <empty scratch dir>}"
here="$(cd "$(dirname "$0")/.." && pwd)"
bin="$here/target/release/bbhost-check"
fixtures="$here/tests/fixtures"

mkdir -p "$demo"
if [ -n "$(ls -A "$demo")" ]; then
  echo "$demo is not empty" >&2
  exit 2
fi
cargo build --release --quiet --manifest-path "$here/Cargo.toml"

export HOME="$demo/home" GIT_CONFIG_NOSYSTEM=1
unset XDG_CONFIG_HOME

# Machine files and repo files come from the fixtures (dot-x -> .x).
(cd "$fixtures" && find . -type f) | while read -r f; do
  case "$f" in */dot-git/*) continue ;; esac
  dest="$demo/$(printf '%s' "$f" | sed 's#/dot-#/.#g')"
  mkdir -p "$(dirname "$dest")"
  cp "$fixtures/$f" "$dest"
done

# Real git repositories, remotes set with git itself.
cd "$demo/code"
git init -q web-app
git -C web-app remote add origin git@bitbucket.org:acme-demo/web-app.git
git -C web-app remote set-url --push origin git@bitbucket.org:acme-demo/web-app.git
git -C web-app remote add mirror https://bitbucket.org/acme-demo/web-app.git
git -C web-app remote add deploy ssh://git@bitbucket.org:22/acme-demo/web-app-deploy.git
git init -q api-service
git -C api-service remote add origin ssh://git@bitbucket.org/acme-demo/api-service.git
git init -q clean-repo
git -C clean-repo remote add origin git@ssh.bitbucket.org:acme-demo/clean-repo.git
git init -q --bare mirrors/legacy.git
git -C mirrors/legacy.git remote add upstream git@bitbucket.org:acme-demo/legacy.git

cd "$demo"
run() { echo "\$ bbhost-check $*"; "$bin" "$@" || echo "(exit $?)"; echo; }
run --root code
run --root code --no-machine --format markdown
echo "\$ echo y | bbhost-check --root code --fix"
echo y | "$bin" --root code --fix || echo "(exit $?)"
echo
echo "\$ git -C code/web-app remote -v"
git -C code/web-app remote -v
