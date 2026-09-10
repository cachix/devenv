#!/usr/bin/env bash

set -xe

output=$(devenv eval cachix.pull)
echo "$output" | jq -e '.["cachix.pull"] | length == 2'
echo "$output" | jq -e '.["cachix.pull"] | sort == ["devenv", "nixpkgs-python"]'
