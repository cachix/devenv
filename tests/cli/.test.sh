#!/usr/bin/env bash
set -euo pipefail

#
# Smoke tests for the devenv CLI surface.
#

step() { echo; echo "── $* ──"; }
fail() { echo "✗ $*" >&2; exit 1; }

rm -f devenv.yaml

step "shell exports cmdline + task-defined env vars"
devenv shell -- env | grep -q DEVENV_CMDLINE
devenv shell -- env | grep -q "DEVENV_CLI_TEST_VAR=hello-from-task"

step "build + inspect a single package attribute"
devenv build languages.python.package

step "shell forwards arguments verbatim and to subdirs"
devenv shell ls -- -la | grep -q "\.test\.sh"
devenv shell ls ../ | grep -q "cli"

step "info/show surface enabled languages"
devenv info | grep -q "python3-"
devenv show | grep -q "python3-"

step "search returns packages and writes trace cache hits"
devenv search ncdu 2>&1 \
  | grep -Eq "Found [0-9]+ packages and [0-9]+ options for 'ncdu'"
RUST_LOG=trace devenv --verbose --trace-output file:search-trace.log \
  search '^ncdu$' 2>&1 \
  | grep -Eq "Found [0-9]+ packages and [0-9]+ options"
grep "cache hit" search-trace.log | grep -q optionsJSON \
  || fail "expected an optionsJSON cache hit in search-trace.log"
devenv search xyznonexistentpackagexyz 2>&1 \
  | grep -Eq "Found 0 packages and 0 options"

step "up fails when no processes are defined"
if devenv up; then fail "devenv up should fail without processes"; fi

step "unknown profile is reported clearly"
out=$(devenv --profile some-profile info 2>&1 || true)
echo "$out" | grep -q "Profile 'some-profile' not found" \
  || fail "expected 'Profile not found' error, got: $out"

step "--from loads an external project and ignores local devenv.nix"
from_test_dir="$(cd from-test && pwd)"
for path in "path:$from_test_dir" "path:./from-test"; do
  out=$(devenv --from "$path" info)
  echo "$out" | grep -q "languages.rust" || fail "--from=$path missing languages.rust"
  ! echo "$out" | grep -q "python3" || fail "--from=$path leaked local python3"
done

step "--from works in a directory without devenv.nix"
mkdir -p test-from-only && pushd test-from-only >/dev/null
out=$(devenv --from "path:$from_test_dir" info)
echo "$out" | grep -q "languages.rust" || fail "--from didn't load remote project"
popd >/dev/null
rm -rf test-from-only

step "-O packages:pkgs appends ad-hoc packages"
devenv -O packages:pkgs "hello" shell -- hello | grep -q "Hello, world"

# Containers are Linux-only.
if [[ "$(uname)" == "Linux" ]]; then
  step "container build fails without required inputs, then succeeds"
  if devenv container build shell; then fail "container build should fail without inputs"; fi
  devenv inputs add mk-shell-bin github:rrbutani/nix-mk-shell-bin --follows nixpkgs
  devenv inputs add nix2container github:nlewo/nix2container --follows nixpkgs
  devenv container build shell | grep -q image-shell.json
  devenv gc
fi
