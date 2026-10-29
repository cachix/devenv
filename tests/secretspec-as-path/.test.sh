#!/usr/bin/env bash
set -euo pipefail

# This is the consuming shell, not the short-lived direnv/devenv exporter.
export DEVENV_SECRETS_OWNER_PID=$$
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

if env -u DEVENV_SECRETS_OWNER_PID devenv print-dev-env > unowned-env.sh 2> unowned-error; then
  echo "as_path export unexpectedly succeeded without an owner" >&2
  exit 1
fi
grep -q 'secret-owner-pid' unowned-error

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

for command in ["print-dev-env", "direnv-export"]:
    script = r'''
set -euo pipefail
devenv "$1" --secret-owner-pid "$$" > child-env.sh
sed -n "s/^TLS_KEY_PATH='\(.*\)'$/\1/p" child-env.sh
read -r _
'''
    owner = subprocess.Popen(["bash", "-c", script, "lease-owner", command],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        path = owner.stdout.readline().strip()
        assert path.startswith("/proc/"), path
        with open(path) as secret:
            assert secret.read() == "production-key"
    finally:
        owner.kill()
        owner.wait()
    wait_for_revocation(path)
PY
echo "✓ anonymous SecretSpec files follow their consuming processes"
