#!/usr/bin/env bash
set -euo pipefail
export DEVENV_SSH_TEST_CLI
DEVENV_SSH_TEST_CLI=$(command -v devenv)
export DEVENV_SSH_TEST_PROJECT="$PWD"
ssh-keygen -q -t ed25519 -N '' -f .ssh-test-key
builds=$(devenv build outputs.ssh-driver outputs.ssh-executor outputs.ssh-confirmation-executor)
driver=$(jq -er '."outputs.ssh-driver"' <<< "$builds")
export DEVENV_SSH_TEST_EXECUTOR
DEVENV_SSH_TEST_EXECUTOR=$(jq -er '."outputs.ssh-executor"' <<< "$builds")
export DEVENV_SSH_TEST_CONFIRMATION_EXECUTOR
DEVENV_SSH_TEST_CONFIRMATION_EXECUTOR=$(jq -er '."outputs.ssh-confirmation-executor"' <<< "$builds")
mkdir -p vm-output
"$driver/bin/nixos-test-driver" --no-interactive -o "$PWD/vm-output"
