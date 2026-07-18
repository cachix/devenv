#!/usr/bin/env bash

set -euo pipefail

export DEVENV_REPO="${DEVENV_REPO:-$(cd ../.. && pwd)}"

test "$(nix-instantiate --eval --strict --impure --argstr repo "$DEVENV_REPO" ./resolver.nix)" = true

test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
target="$test_root/target"
runtime="$test_root/runtime"
mkdir -m 755 "$target"
ln -s "$target" "$runtime"

coreutils="$(dirname "$(dirname "$(command -v chmod)")")"
prepare="$(
  COREUTILS="$coreutils" RUNTIME="$runtime" nix eval --impure --raw --expr '
    let
      repo = builtins.getEnv "DEVENV_REPO";
      runtimeDir = import (builtins.toPath "${repo}/src/modules/runtime-dir.nix");
    in runtimeDir.prepare {
      coreutils = builtins.getEnv "COREUTILS";
      runtime = builtins.getEnv "RUNTIME";
    }
  '
)"

if bash -c "$prepare"; then
  echo "symlinked runtime directory was accepted" >&2
  exit 1
fi

test "$(stat --format=%a "$target")" = 755
