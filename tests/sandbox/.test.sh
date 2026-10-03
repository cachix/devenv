#!/usr/bin/env bash
set -euo pipefail

# Standard temporary directories are writable under nono's baseline policy.
# Put the denied fixture under HOME, outside the project and private TMPDIR.
fixture=$(mktemp -d "$HOME/devenv-sandbox-test.XXXXXX")
export TMPDIR="$fixture/tmp"
export SANDBOX_OUTSIDE="$fixture/outside"
mkdir -p "$TMPDIR" "$SANDBOX_OUTSIDE"
trap 'rm -rf "$fixture"; rm -f shell-result task-result process-result status-result status-leak sandbox.typescript interactive-denied interactive-leak' EXIT
printf secret > "$SANDBOX_OUTSIDE/secret"

devenv shell -- bash -c '
  printf shell > "$DEVENV_ROOT/shell-result"
  ! cat "$SANDBOX_OUTSIDE/secret" && ! touch "$SANDBOX_OUTSIDE/shell-write"
'
test "$(cat shell-result)" = shell

devenv tasks run sandbox:check
test "$(cat task-result)" = task

devenv tasks run sandbox:status
test -f status-result
test ! -e status-leak

devenv tasks run sandbox:service
test "$(cat process-result)" = process

# The PTY path uses the reload coordinator's interactive shell launcher.
devenv-run-tests pty sandbox.typescript "devenv shell" >/dev/null <<'EOF_PTY'
expect:DEVENV_SHELL_READY
send:if cat "$SANDBOX_OUTSIDE/secret"; then touch interactive-leak; else touch interactive-denied; fi; echo INTERACTIVE_DONE\n
expect:INTERACTIVE_DONE
send:exit\n
EOF_PTY
test -f interactive-denied
test ! -e interactive-leak

# Exit codes survive the exec-based wrapper.
status=0
devenv shell -- bash -c 'exit 23' || status=$?
test "$status" -eq 23

# Explicitly disabling the sandbox restores ordinary execution.
printf 'sandbox: false\n' > devenv.local.yaml
trap 'rm -rf "$fixture"; rm -f shell-result task-result process-result status-result status-leak sandbox.typescript interactive-denied interactive-leak devenv.local.yaml' EXIT
devenv shell -- cat "$SANDBOX_OUTSIDE/secret"
