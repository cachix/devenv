#!/usr/bin/env bash
set -xe
set -o pipefail

# Test devenv integrated into bare Nix flake
nix flake init --template "${DEVENV_REPO}"
nix flake update --override-input devenv "${DEVENV_REPO}"

# Test that nix develop works with --no-pure-eval
nix develop --accept-flake-config --no-pure-eval \
  --override-input devenv "$DEVENV_REPO" \
  --command echo nix-develop started successfully 2>&1 | tee ./console
grep -F 'nix-develop started successfully' <./console
grep -F 'Hello, world!' <./console

# The flakes-integration default must remain independent of TMPDIR.
project_root="$(pwd -P)"
mkdir -p "$project_root/tmp-a" "$project_root/tmp-b"
runtime_a="$(
  env -u DEVENV_RUNTIME -u XDG_RUNTIME_DIR TMPDIR="$project_root/tmp-a" \
    nix develop --accept-flake-config --no-pure-eval \
    --override-input devenv "$DEVENV_REPO" --command \
    sh -c 'printf "%s\n" "$DEVENV_RUNTIME"' | tail -n 1
)"
runtime_b="$(
  env -u DEVENV_RUNTIME -u XDG_RUNTIME_DIR TMPDIR="$project_root/tmp-b" \
    nix develop --accept-flake-config --no-pure-eval \
    --override-input devenv "$DEVENV_REPO" --command \
    sh -c 'printf "%s\n" "$DEVENV_RUNTIME"' | tail -n 1
)"
short_hash="$(printf %s "$project_root/.devenv" | sha256sum | cut -c1-7)"
expected_runtime="/tmp/devenv-$short_hash"
test "$runtime_a" = "$runtime_b"
test "$runtime_a" = "$expected_runtime"

inherited_runtime="$(
  env -u XDG_RUNTIME_DIR DEVENV_RUNTIME="$project_root/custom-runtime" \
    nix develop --accept-flake-config --no-pure-eval \
    --override-input devenv "$DEVENV_REPO" --command \
    sh -c 'printf "%s\n" "$DEVENV_RUNTIME"' | tail -n 1
)"
test "$inherited_runtime" = "$expected_runtime"

xdg_runtime_dir="$project_root/xdg-runtime"
configured_runtime="$(
  env XDG_RUNTIME_DIR="$xdg_runtime_dir" DEVENV_RUNTIME="$project_root/custom-runtime" \
    nix develop --accept-flake-config --no-pure-eval \
    --override-input devenv "$DEVENV_REPO" --command \
    sh -c 'printf "%s\n" "$DEVENV_RUNTIME"' | tail -n 1
)"
test "$configured_runtime" = "$xdg_runtime_dir/devenv-$short_hash"

# Assert that nix-develop fails in pure mode
if nix develop --command echo nix-develop started in pure mode 2>&1 | tee ./console
then
  echo "nix-develop was able to start in pure mode. This is explicitly not supported."
  exit 1
fi
grep -F 'devenv was not able to determine the current directory.' <./console
