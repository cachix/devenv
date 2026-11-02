#!/usr/bin/env bash
set -euo pipefail
export DEVENV_SSH_TEST_CLI
DEVENV_SSH_TEST_CLI=$(command -v devenv)
export DEVENV_SSH_TEST_PROJECT="$PWD"
ssh-keygen -q -t ed25519 -N '' -f .ssh-test-key
printf '%s\n' 'APP_TOKEN=token:one!' 'AGE_KEY=age-token:one!' 'EXTRA_TOKEN=extra-token:one!' > .env
builds=$(devenv build outputs.secrets-driver outputs.secrets-executor outputs.secrets-failing-executor)
driver=$(jq -er '."outputs.secrets-driver"' <<< "$builds")
export DEVENV_SSH_TEST_EXECUTOR
DEVENV_SSH_TEST_EXECUTOR=$(jq -er '."outputs.secrets-executor"' <<< "$builds")
export DEVENV_SSH_TEST_FAILING_EXECUTOR
DEVENV_SSH_TEST_FAILING_EXECUTOR=$(jq -er '."outputs.secrets-failing-executor"' <<< "$builds")
mkdir -p vm-output
runtime=$(mktemp -d)
trap 'rm -rf "$runtime"' EXIT
XDG_RUNTIME_DIR="$runtime" "$driver/bin/nixos-test-driver" --no-interactive -o "$PWD/vm-output"
