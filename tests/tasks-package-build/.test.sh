#!/usr/bin/env bash

# Test that the user's nixpkgs does not affect the tasks package build.
# This ensures that devenv-tasks is built using a locked nixpkgs input.

set -xe

tasks_path=$(devenv build task.package)
tasks_path_unstable=$(devenv build task.package --override-input nixpkgs github:nixos/nixpkgs/nixpkgs-unstable)

if [ "$tasks_path" != "$tasks_path_unstable" ]; then
  echo "FAILED: Store path mismatch in devenv-tasks"
  echo "Expected: $tasks_path"
  echo "Got from unstable: $tasks_path_unstable"
  exit 1
else
  echo "SUCCESS: devenv-tasks store path not affected by user's nixpkgs"
fi
