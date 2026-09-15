#!/usr/bin/env bash
set -eu

devenv-run-tests pty shell.typescript "devenv shell" >/dev/null <<'EOF'
expect:DEVENV_SHELL_READY
send:./alternate-screen-reader.sh\n
expect:READY
send:\033\004
expect:RECEIVED: 1b 04
send:exit\n
EOF

grep -aFq "RECEIVED: 1b 04" shell.typescript
