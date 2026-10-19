#!/usr/bin/env bash
set -euo pipefail

test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=test GIT_COMMITTER_NAME=test
export GIT_AUTHOR_EMAIL=test@example.com GIT_COMMITTER_EMAIL=test@example.com

cd "$test_dir"
git init -q
mergiraf --version
test "$(git config merge.conflictStyle)" = diff3
test "$(git config merge.mergiraf.name)" = mergiraf
test "$(git check-attr merge -- example.json)" = 'example.json: merge: mergiraf'
# Registering a driver must not select an interactive mergetool.
if git config --get merge.tool; then
  echo 'Mergiraf unexpectedly selected an interactive merge tool' >&2
  exit 1
fi

printf '{"base":0}\n' > example.json
git add example.json
git commit -qm base
git branch side
printf '{"base":0,"ours":1}\n' > example.json
git commit -qam ours
git checkout -q side
printf '{"base":0,"theirs":2}\n' > example.json
git commit -qam theirs
git checkout -q -

# The same-line additions conflict with Git's text merge, but can be combined
# structurally. First verify the fixture really requires the merge driver.
if mergiraf=0 git merge --no-edit side; then
  echo 'Expected a text merge conflict' >&2
  exit 1
fi
git merge --abort
git merge --no-edit side
jq -e '. == {base: 0, ours: 1, theirs: 2}' example.json
