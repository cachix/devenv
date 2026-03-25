#!/usr/bin/env bash
set -ex

output_dir=$DEVENV_ROOT/docs/src/_generated/reference
mkdir -p "$output_dir"

options=$(devenv-build outputs.devenv-docs-options)

{
  echo "# devenv.nix"
  echo
  cat "$options"
} > "$output_dir/options.md"

# https://github.com/NixOS/nixpkgs/issues/224661
sed -i 's/\\\././g' "$output_dir/options.md"
