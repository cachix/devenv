#!/usr/bin/env bash
set -ex

# Verify that the env var exported by the enterShell task is injected into
# the test script environment by prepare_shell().
test "$DEVENV_TEST_VAR" = "hello-from-task"
