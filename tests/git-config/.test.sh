#!/usr/bin/env bash
set -euo pipefail

# Use isolated user configuration, while retaining devenv's runtime settings.
test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
export HOME="$test_dir/home"
export XDG_CONFIG_HOME="$HOME/.config"
export GIT_CONFIG_NOSYSTEM=1
unset GIT_CONFIG_GLOBAL
mkdir -p "$XDG_CONFIG_HOME/git"
printf '[test]\n inherited = xdg\n' > "$XDG_CONFIG_HOME/git/config"
printf '[test]\n inherited = home\n[merge]\n tool = meld\n' > "$HOME/.gitconfig"

cd "$test_dir"
git init -q repo
cd repo

test "$(git config test.inherited)" = home
printf '[test]\n inherited = custom\n' > "$test_dir/custom-global"
test "$(GIT_CONFIG_GLOBAL="$test_dir/custom-global" git config test.inherited)" = custom
test "$(git config merge.tool)" = vimdiff
test "$(git config merge.conflictStyle)" = diff3
test "$(git config --bool test.boolean)" = true
test "$(git config --int test.number)" = 42
test "$(git config test.quoted)" = 'spaces, "quotes", and \backslashes'
test "$(git config --get-all test.repeated)" = "$(printf 'first\nsecond')"

# Runtime settings override local files; explicit command options override them.
git config --local merge.tool meld
test "$(git config merge.tool)" = vimdiff
test "$(git -c merge.tool=custom config merge.tool)" = custom

test "$(git check-attr merge -- example.txt)" = 'example.txt: merge: test'
test "$(git check-attr diff -- example.dat)" = 'example.dat: diff: unset'
printf '*.txt merge=text\n' > .gitattributes
test "$(git check-attr merge -- example.txt)" = 'example.txt: merge: text'
printf '*.txt merge=binary\n' > .git/info/attributes
test "$(git check-attr merge -- example.txt)" = 'example.txt: merge: binary'
rm .gitattributes .git/info/attributes

# Exercise a real merge with the configured driver selected by the attributes.
export GIT_AUTHOR_NAME=test GIT_COMMITTER_NAME=test
export GIT_AUTHOR_EMAIL=test@example.com GIT_COMMITTER_EMAIL=test@example.com
printf 'base\n' > example.txt
git add example.txt
git commit -qm base
git branch side
printf 'ours\n' > example.txt
git commit -qam ours
git checkout -q side
printf 'theirs\n' > example.txt
git commit -qam theirs
git checkout -q -
git merge -q --no-edit side
test "$(cat example.txt)" = theirs

# Nothing was persisted in either user or repository configuration.
test "$(git config --local merge.tool)" = meld
if env -u GIT_CONFIG_COUNT -u GIT_CONFIG_KEY_0 -u GIT_CONFIG_VALUE_0 git config --get merge.test.driver; then
  echo 'devenv configuration leaked outside the environment' >&2
  exit 1
fi
