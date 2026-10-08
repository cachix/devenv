#!/usr/bin/env bash
set -euo pipefail

# Standard temporary directories are writable under nono's baseline policy.
# Put the denied fixture under HOME, outside the project and private TMPDIR.
fixture=$(mktemp -d "$HOME/devenv-sandbox-test.XXXXXX")
export TMPDIR="$fixture/tmp"
export SANDBOX_OUTSIDE="$fixture/outside"
mkdir -p "$TMPDIR" "$SANDBOX_OUTSIDE"
network_server=
trap 'if [[ -n "$network_server" ]]; then kill "$network_server" || true; wait "$network_server" || true; fi; rm -rf "$fixture"; rm -f shell-result task-result process-result status-result status-leak sandbox.typescript interactive-denied interactive-leak extra-read extra-check.sh network-port network.typescript network-interactive startup-check.sh devenv.local.yaml' EXIT
printf secret > "$SANDBOX_OUTSIDE/secret"

# Startup code may run after sandboxing, but cannot write to an ungranted path.
cat > startup-check.sh <<'EOF_STARTUP'
if printf leaked > "$SANDBOX_OUTSIDE/startup-leak" 2>/dev/null; then
  printf 'Unsandboxed startup in %s, pid %s\n' "$0" "$$" >&2
  exit 91
fi
EOF_STARTUP
# The test harness puts a Bash wrapper ahead of the actual CLI. Protect that
# wrapper too, so the inherited startup code reaches the CLI under test.
devenv_wrapper=$(command -v devenv)
BASH_ENV="$PWD/startup-check.sh" bash -p "$devenv_wrapper" shell -- true
test ! -e "$SANDBOX_OUTSIDE/startup-leak"
BASH_ENV="$PWD/startup-check.sh" bash -p "$devenv_wrapper" tasks run sandbox:status
test ! -e "$SANDBOX_OUTSIDE/startup-leak"
BASH_ENV="$PWD/startup-check.sh" bash -p "$devenv_wrapper" tasks run sandbox:service
test ! -e "$SANDBOX_OUTSIDE/startup-leak"
rm -f status-result process-result

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

# Project and user grants combine for commands and tasks, including file grants.
export EXTRA_READ_DIR="$fixture/project read" EXTRA_READ_FILE="$fixture/read-only-file"
export EXTRA_WRITE_DIR="$fixture/project write" GLOBAL_READ_DIR="$fixture/global read"
export GLOBAL_WRITE_FILE="$fixture/global-write-file"
mkdir -p "$EXTRA_READ_DIR" "$EXTRA_WRITE_DIR" "$GLOBAL_READ_DIR"
printf project > "$EXTRA_READ_DIR/secret"
printf file > "$EXTRA_READ_FILE"
printf global > "$GLOBAL_READ_DIR/secret"
touch "$GLOBAL_WRITE_FILE"
ln -s "$EXTRA_READ_DIR" extra-read
cat > devenv.local.yaml <<EOF_PROJECT
sandbox:
  read: [extra-read, "$EXTRA_READ_FILE"]
  write: ["$EXTRA_WRITE_DIR"]
EOF_PROJECT
cat > "$fixture/user.yaml" <<'EOF_USER'
version: 1
sandbox:
  read: ['global read']
  write: [global-write-file]
EOF_USER
cat > extra-check.sh <<'EOF_CHECK'
set -euo pipefail
cat "$EXTRA_READ_DIR/secret"
cat "$EXTRA_READ_FILE"
cat "$GLOBAL_READ_DIR/secret"
printf '%s' "$1" > "$EXTRA_WRITE_DIR/$1"
printf '%s' "$1" > "$GLOBAL_WRITE_FILE"
if touch "$EXTRA_READ_DIR/forbidden"; then exit 1; fi
if printf forbidden > "$EXTRA_READ_FILE"; then exit 1; fi
if cat "$SANDBOX_OUTSIDE/secret"; then exit 1; fi
EOF_CHECK
devenv --user-config "$fixture/user.yaml" shell -- bash ./extra-check.sh shell
test "$(cat "$EXTRA_WRITE_DIR/shell")" = shell
test "$(cat "$GLOBAL_WRITE_FILE")" = shell
devenv --user-config "$fixture/user.yaml" tasks run sandbox:extra
test "$(cat "$EXTRA_WRITE_DIR/task")" = task
test "$(cat "$GLOBAL_WRITE_FILE")" = task
test "$(cat "$EXTRA_READ_FILE")" = file

# Networking is enabled by default; exercise a real listener without external services.
python_path=$(devenv shell -- which python3 | tail -n 1)
"$python_path" network-check.py server &
network_server=$!
for attempt in {1..100}; do
  if [[ -s network-port ]]; then break; fi
  sleep 0.05
done
test -s network-port
export SANDBOX_NETWORK_PORT=$(cat network-port)
devenv shell -- python3 ./network-check.py allowed

# A project can block networking for commands, tasks, and interactive reload shells.
cat > devenv.local.yaml <<'EOF_NETWORK'
sandbox:
  networking:
    enable: false
EOF_NETWORK
devenv shell -- python3 ./network-check.py blocked
devenv tasks run sandbox:network
devenv-run-tests pty network.typescript "devenv shell" >/dev/null <<'EOF_NETWORK_PTY'
expect:DEVENV_SHELL_READY
send:python3 ./network-check.py blocked && touch network-interactive; echo NETWORK_DONE\n
expect:NETWORK_DONE
send:exit\n
EOF_NETWORK_PTY
test -f network-interactive

# User networking defaults apply to noninteractive commands too.
cat > "$fixture/network-user.yaml" <<'EOF_NETWORK_USER'
version: 1
sandbox:
  enable: true
  networking:
    enable: false
EOF_NETWORK_USER
rm devenv.local.yaml
cp devenv.yaml "$fixture/project.yaml"
printf '{}\n' > devenv.yaml
devenv --user-config "$fixture/network-user.yaml" shell -- python3 ./network-check.py blocked
cp "$fixture/project.yaml" devenv.yaml

# An explicit project setting overrides the user's network default.
cat > devenv.local.yaml <<'EOF_NETWORK_OVERRIDE'
sandbox:
  networking:
    enable: true
EOF_NETWORK_OVERRIDE
devenv --user-config "$fixture/network-user.yaml" shell -- python3 ./network-check.py allowed

# A nonexistent grant fails before executing the requested command.
printf 'sandbox:\n  read: ["%s/missing"]\n' "$fixture" > devenv.local.yaml
if devenv --user-config "$fixture/user.yaml" shell -- touch "$EXTRA_WRITE_DIR/should-not-run"; then
  echo "Missing sandbox path was accepted" >&2
  exit 1
fi
test ! -e "$EXTRA_WRITE_DIR/should-not-run"

# Explicitly disabling the sandbox restores ordinary execution.
printf 'sandbox:\n  enable: false\n' > devenv.local.yaml
devenv --user-config "$fixture/network-user.yaml" shell -- cat "$SANDBOX_OUTSIDE/secret"
devenv --user-config "$fixture/network-user.yaml" shell -- python3 ./network-check.py allowed
