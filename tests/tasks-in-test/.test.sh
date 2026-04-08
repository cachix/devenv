#!/usr/bin/env bash
set -ex

# Verify that env vars exported by enterShell and enterTest tasks are
# injected into the test script environment by prepare_shell().

# Basic export
test "$DEVENV_TEST_VAR" = "hello-from-task"

# Multiple exports from one task
test "$DEVENV_TEST_MULTI" = "second-var"

# Empty string value (verify it is set, not just absent)
test "${DEVENV_TEST_EMPTY+is_set}" = "is_set"
test "$DEVENV_TEST_EMPTY" = ""

# Value with spaces
test "$DEVENV_TEST_SPACES" = "hello world with spaces"

# Value containing equals signs
test "$DEVENV_TEST_EQUALS" = "key=value=more"

# Export from a second task (tests merging across tasks)
test "$DEVENV_TEST_FROM_SECOND" = "from-second-task"

# Variable not in exports list should not be present
test -z "${DEVENV_TEST_NOT_EXPORTED:-}"

# enterTest-only task should have run (devenv test runs enterTest root,
# which depends on enterShell)
test "$DEVENV_TEST_ENTER_TEST_RAN" = "yes"

# Task depending on a process with exec readiness probe should have run.
# Regression test for https://github.com/cachix/devenv/issues/2713
test "$DEVENV_TEST_PROCESS_WAS_READY" = "yes"
