#!/usr/bin/env bash
set -e

# First run devenv to create the config file (hooks enabled)
devenv shell true

# Verify the config file was created
if ! test -f ".pre-commit-config.yaml"; then
  echo "Test not setup correctly: .pre-commit-config.yaml not found" >&2
  exit 1
fi

# Now disable the hook - the next devenv evaluation should remove the config
echo "{ lib, ... }: { git-hooks.hooks.no-op.enable = lib.mkForce false; }" > devenv.local.nix
