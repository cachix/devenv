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
# from-test enables languages.rust, this project enables languages.python.
# Pulling rust-toolchain into the package list while dropping python3 proves
# we used the --from devenv.nix, not the local one.
for path in "path:$from_test_dir" "path:./from-test"; do
  out=$(devenv --from "$path" info)
  echo "$out" | grep -q "rust-toolchain" || fail "--from=$path missing rust toolchain"
  ! echo "$out" | grep -q "python3" || fail "--from=$path leaked local python3"
done

step "--from loads YAML imports from a fetched source"
fetched_source_repo="$(mktemp -d "${TMPDIR:-/tmp}/devenv-from-source.XXXXXX")"
fetched_source_remote="$(mktemp -d "${TMPDIR:-/tmp}/devenv-from-remote.XXXXXX")"
mkdir -p "$fetched_source_repo/configs/source/modules/marker"
cat > "$fetched_source_repo/configs/source/devenv.nix" <<'EOF'
{ ... }: { }
EOF
cat > "$fetched_source_repo/configs/source/devenv.yaml" <<'EOF'
imports:
  - ./modules/marker
EOF
cat > "$fetched_source_repo/configs/source/modules/marker/devenv.nix" <<'EOF'
{ ... }:
{
  env.FETCHED_FROM_YAML_IMPORT = "loaded-from-yaml-import";
}
EOF
git -C "$fetched_source_repo" init -q
git -C "$fetched_source_repo" config user.email "tests@devenv.sh"
git -C "$fetched_source_repo" config user.name "devenv tests"
git -C "$fetched_source_repo" config commit.gpgsign false
git -C "$fetched_source_repo" add .
git -C "$fetched_source_repo" commit -q -m "Add fetched source fixture"
git -C "$fetched_source_remote" init -q --bare
git -C "$fetched_source_repo" remote add origin "$fetched_source_remote"
git -C "$fetched_source_repo" push -q -u origin HEAD
fetched_source="git+file://$fetched_source_remote?dir=configs/source"

fetched_source_failures=0
if ! out=$(devenv --from "$fetched_source" shell -- sh -c 'printf %s "$FETCHED_FROM_YAML_IMPORT"') \
  || [[ "$out" != *"loaded-from-yaml-import"* ]]; then
  echo "direct fetched source did not load its YAML import" >&2
  fetched_source_failures=$((fetched_source_failures + 1))
fi

if out=$(devenv --from "$fetched_source" \
  --override-input from "$fetched_source" info 2>&1); then
  echo "fetched source accepted a from input override" >&2
  fetched_source_failures=$((fetched_source_failures + 1))
elif [[ "$out" != *"Input from does not exist so it can't be overridden."* ]]; then
  echo "fetched source returned an unexpected from input override error: $out" >&2
  fetched_source_failures=$((fetched_source_failures + 1))
fi

fetched_target="$(mktemp -d "${TMPDIR:-/tmp}/devenv-from-target.XXXXXX")"
pushd "$fetched_target" >/dev/null
mkdir fixture
cat > devenv.yaml <<'EOF'
inputs:
  fixture:
    url: path:./fixture
    flake: false
EOF
devenv --from "$fetched_source" allow
if ! out=$(devenv shell -- sh -c 'printf %s "$FETCHED_FROM_YAML_IMPORT"') \
  || [[ "$out" != *"loaded-from-yaml-import"* ]]; then
  echo "persisted fetched source did not load its YAML import" >&2
  fetched_source_failures=$((fetched_source_failures + 1))
fi
cat >> "$fetched_source_repo/configs/source/devenv.yaml" <<'EOF'
require_version: ">=999.0"
EOF
git -C "$fetched_source_repo" add configs/source/devenv.yaml
git -C "$fetched_source_repo" commit -q -m "Change fetched source fixture"
git -C "$fetched_source_repo" push -q
if ! devenv update fixture; then
  echo "updating another input refreshed the fetched source" >&2
  fetched_source_failures=$((fetched_source_failures + 1))
fi
devenv revoke
popd >/dev/null
rm -rf "$fetched_target" "$fetched_source_repo" "$fetched_source_remote"

(( fetched_source_failures == 0 )) \
  || fail "$fetched_source_failures fetched --from checks failed"

step "--from ignores the local devenv.local.nix"
mkdir -p test-local-override && pushd test-local-override >/dev/null
cat > devenv.nix <<'EOF'
{ ... }:
{
  processes.local-only.exec = "sleep 100";
}
EOF
# A local devenv.local.nix must not be merged into an external --from project.
# It references a process defined only by the local devenv.nix, so if it leaks
# into the external project the eval fails on an undefined `exec`.
cat > devenv.local.nix <<'EOF'
{ ... }:
{
  processes.local-only.process-compose.disabled = true;
}
EOF
out=$(devenv --from "path:$from_test_dir" info) \
  || fail "--from failed with a local devenv.local.nix present"
echo "$out" | grep -q "rust-toolchain" \
  || fail "--from=$from_test_dir missing rust toolchain"
popd >/dev/null
rm -rf test-local-override

step "--from works in a directory without devenv.nix"
mkdir -p test-from-only && pushd test-from-only >/dev/null
out=$(devenv --from "path:$from_test_dir" info)
echo "$out" | grep -q "rust-toolchain" || fail "--from didn't load remote project"
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
