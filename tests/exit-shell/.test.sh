#!/usr/bin/env bash
set -eu

# Non-interactive commands must not run exitShell
devenv shell -- true
if [ -e exit-shell-ran ] || [ -e before-exit-ran ]; then
  echo "exitShell ran for a non-interactive command"
  exit 1
fi

devenv-run-tests pty shell.typescript "devenv shell" >/dev/null <<'PTY'
expect:DEVENV_SHELL_READY
send:exit\n
PTY

test -e exit-shell-ran
test -e before-exit-ran
