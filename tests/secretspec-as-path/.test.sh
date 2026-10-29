#!/usr/bin/env bash
set -euo pipefail

daemon_started=0
cleanup() {
  if (( daemon_started )); then
    devenv processes down || true
  fi
}
trap cleanup EXIT

load_key_path() {
  devenv "$@" print-dev-env > env.sh
  sed -n "s/^TLS_KEY_PATH='\(.*\)'$/\1/p" env.sh
}

path=$(load_key_path)
case "$path" in /proc/*/fd/*) ;; *) echo "not an anonymous file: $path" >&2; exit 1 ;; esac
test "$(cat "$path")" = "test-key"

# Every resolution is independent, including reloads of the same profile.
printf 'TLS_KEY=production-key\n' > .env
production_path=$(load_key_path --secretspec-profile production)
test "$path" != "$production_path"
test "$(cat "$path")" = "test-key"
test "$(cat "$production_path")" = "production-key"

printf 'TLS_KEY=other-store-key\n' > .env.other
other_path=$(load_key_path --secretspec-provider dotenv:.env.other)
test "$path" != "$other_path"
test "$(cat "$path")" = "test-key"
test "$(cat "$other_path")" = "other-store-key"

# exec must preserve the anonymous-file lease until the command exits.
devenv shell -- bash -c 'test "$(cat "$TLS_KEY_PATH")" = "production-key"; printf "%s\n" "$TLS_KEY_PATH" > exec-key-path'

# A detached manager must take over the launching CLI's lease.
devenv up -d
daemon_started=1
for _ in {1..100}; do
  test ! -f daemon-key-path || break
  sleep 0.1
done
test -f daemon-key-path
test "$(cat "$(cat daemon-key-path)")" = "production-key"
devenv processes down
daemon_started=0

# Exercise the real direnv export path using the ordinary .envrc integration.
test_shells=$(devenv build outputs.test-shells | jq -er '."outputs.test-shells"')
export PATH="$test_shells/bin:$PATH"
# Zsh's system startup files may replace PATH even with user startup disabled.
# Pin the binaries under test before starting any consuming shell.
export DEVENV_BIN="$(command -v devenv)"
export TEST_DIRENV_BIN="$(command -v direnv)"
cat > .envrc <<'ENVRC'
eval "$("$DEVENV_BIN" direnvrc)"
use devenv
ENVRC
direnv allow

# Both exporters must tie their leases to the actual consumer, even when it
# exits without running shell traps. The keeper must not retain stdout pipes.
python3 - <<'PY'
import os
import subprocess
import time


def wait_for_revocation(path):
    deadline = time.monotonic() + 5
    while os.path.exists(path) and time.monotonic() < deadline:
        time.sleep(0.01)
    assert not os.path.exists(path), f"secret outlived its owner: {path}"


for path_file in ["exec-key-path", "daemon-key-path"]:
    with open(path_file) as source:
        wait_for_revocation(source.read().strip())

scripts = [
    r"""
set -euo pipefail
load_env() { devenv "$1"; }
env_output=$(load_env "$1")
printf '%s\n' "$env_output" > child-env.sh
sed -n "s/^TLS_KEY_PATH='\(.*\)'$/\1/p" child-env.sh
printf '%s\n' "$$"
read -r _
""",
    r"""
set -euo pipefail
# Keep the command substitution alive after direnv exits, exercising a Bash
# fork between direnv and the shell whose prompt consumes the environment.
load_env() { "$TEST_DIRENV_BIN" export bash; :; }
eval "$(load_env)"
printf '%s\n' "$TLS_KEY_PATH" "$$"
read -r _
""",
]

for shell, option, script, command in [
        ("bash", "--norc", scripts[0], "print-dev-env"),
        ("bash", "--norc", scripts[0], "direnv-export"),
        ("bash", "--norc", scripts[1], "direnv"),
        ("zsh", "-f", r'''
set -eu
eval "$("$TEST_DIRENV_BIN" export zsh)"
printf '%s\n' "$TLS_KEY_PATH" "$$"
read -r _
''', "direnv"),
        ("fish", "--no-config", r'''
"$TEST_DIRENV_BIN" export fish | source
printf '%s\n' "$TLS_KEY_PATH" "$fish_pid"
read -l answer
''', "direnv"),
        ("bash", "--norc", r'''
set -euo pipefail
"$TEST_DIRENV_BIN" exec . bash -c '
  printf "%s\n" "$TLS_KEY_PATH" "$$"
  read -r _
'
read -r _
''', "direnv-exec")]:
    # A new exec'd shell must own the lease, even with a live outer shell.
    outer = r' "$3" "$4" -c "$1" lease-owner "$2"; read -r _ '
    owner = subprocess.Popen(["bash", "-c", outer, "outer-shell", script, command, shell, option],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                             start_new_session=True)
    try:
        path = owner.stdout.readline().strip()
        shell_pid = int(owner.stdout.readline().strip())
        assert path.startswith("/proc/"), path
        with open(path) as secret:
            assert secret.read() == "production-key"
        os.kill(shell_pid, 9)
        wait_for_revocation(path)
        assert owner.poll() is None, "outer shell should still be alive"
    finally:
        os.killpg(owner.pid, 9)
        owner.wait()
PY
echo "✓ anonymous SecretSpec files follow their consuming processes"
