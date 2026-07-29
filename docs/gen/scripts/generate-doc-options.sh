#!/usr/bin/env bash

set -ex

output_file=../src/data/options.json

# Build options using the docs/gen devenv environment
options=$(devenv-build outputs.devenv-docs-options-json)

mkdir -p "$(dirname "$output_file")"
if [[ -e "$output_file" ]]; then
  chmod u+w "$output_file"
fi
cp --no-preserve=mode,timestamps "$options/share/doc/nixos/options.json" "$output_file"
