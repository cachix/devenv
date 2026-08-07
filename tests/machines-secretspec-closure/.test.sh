#!/usr/bin/env bash
set -euo pipefail

build_json=$(devenv build machines.resolver.build.nixos)
toplevel=$(jq -er '."machines.resolver.build.nixos"' <<<"$build_json")
launcher="$toplevel/sw/bin/devenv-machines-secretspec"

if [[ ! -x "$launcher" ]]; then
  echo "target NixOS closure does not contain the SecretSpec launcher: $launcher"
  exit 1
fi

launcher_path=$(readlink -f "$launcher")
if ! grep -q -- '-devenv-bundled-secretspec/bin/secretspec' "$launcher_path"; then
  echo "target launcher does not reference devenv's bundled SecretSpec"
  cat "$launcher_path"
  exit 1
fi

expected_version=$(secretspec --version)
actual_version=$("$launcher" --version)
if [[ "$actual_version" != "$expected_version" ]]; then
  echo "target resolver version differs from the SecretSpec shipped with devenv"
  echo "expected: $expected_version"
  echo "actual:   $actual_version"
  exit 1
fi

echo "built $toplevel with $actual_version"
