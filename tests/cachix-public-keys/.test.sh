#!/usr/bin/env bash

set -xe

# Start from a devenv home without cached keys so the fetch has to happen.
DEVENV_HOME="$(mktemp -d)"
export DEVENV_HOME

devenv eval cachix.pull | jq -e '.["cachix.pull"] | index("devenv") != null'

jq -e '.devenv | startswith("devenv.cachix.org-1:")' "$DEVENV_HOME/cachix_trusted_keys.json"
