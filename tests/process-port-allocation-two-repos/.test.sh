#!/usr/bin/env bash
set -euo pipefail

. "$DEVENV_TEST_LIB"

cleanup() {
  for repo in repo2 repo1; do
    if [ -d "$repo" ]; then
      (cd "$repo" && devenv processes down >/dev/null 2>&1) || true
    fi
    if [ -f "$repo/up.pid" ]; then
      kill "$(cat "$repo/up.pid")" 2>/dev/null || true
      wait "$(cat "$repo/up.pid")" 2>/dev/null || true
      rm -f "$repo/up.pid"
    fi
    if [ -f "$repo/server.pid" ]; then
      kill "$(cat "$repo/server.pid")" 2>/dev/null || true
      rm -f "$repo/server.pid"
    fi
  done
}
trap cleanup EXIT

base_port=$(python3 - <<'PY'
import errno
import socket

def can_allocate(port):
    listeners = []
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.bind(("0.0.0.0", port))
        sock.listen(1)
        listeners.append(sock)

        try:
            sock6 = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
            sock6.bind(("::1", port))
            sock6.listen(1)
            listeners.append(sock6)
        except OSError as error:
            if error.errno not in {
                errno.EADDRNOTAVAIL,
                errno.EAFNOSUPPORT,
                errno.EPROTONOSUPPORT,
            }:
                return False

        return True
    except OSError:
        return False
    finally:
        for listener in listeners:
            listener.close()

for _ in range(1000):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    if port < 65436 and can_allocate(port):
        print(port)
        break
else:
    raise SystemExit("could not find an allocatable base port")
PY
)

make_repo() {
  local repo=$1
  mkdir -p "$repo"
  cat > "$repo/server.py" <<'PY'
import socket
import sys
import os
from pathlib import Path

port = int(sys.argv[1])

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", port))
    Path("server.pid").write_text(f"{os.getpid()}\n")
    Path("allocated-port").write_text(f"{port}\n")
    sock.listen(16)
    while True:
        conn, _addr = sock.accept()
        conn.close()
PY
  cat > "$repo/devenv.nix" <<'NIX'
{ config, lib, pkgs, ... }:
let
  allocatedPort = config.processes.web.ports.http.value;
in
{
  env.ALLOCATED_PORT = toString allocatedPort;

  processes.web = {
    ports.http.allocate = __BASE_PORT__;
    restart.on = "never";
    ready.exec = "test -s allocated-port";
    exec = ''
      exec ${lib.getExe pkgs.python3} server.py ${toString allocatedPort}
    '';
  };
}
NIX
  python3 - "$repo/devenv.nix" "$base_port" <<'PYEDIT'
from pathlib import Path
import sys

path = Path(sys.argv[1])
path.write_text(path.read_text().replace("__BASE_PORT__", sys.argv[2]))
PYEDIT
}

# These probe a TCP port rather than an HTTP endpoint, so they are local. The
# names stay clear of devenv's own `wait_for_port`, which takes a timeout and
# checks something else.
tcp_is_ready() {
  (echo > "/dev/tcp/127.0.0.1/$1") >/dev/null 2>&1
}

tcp_is_gone() {
  ! tcp_is_ready "$1"
}

port_is_allocatable() {
  local port=$1
  python3 - "$port" <<'PY' >/dev/null 2>&1
import socket
import sys
import errno

port = int(sys.argv[1])
listeners = []
try:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("0.0.0.0", port))
    sock.listen(1)
    listeners.append(sock)

    sock6 = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
    sock6.bind(("::1", port))
    sock6.listen(1)
    listeners.append(sock6)
except OSError as error:
    if error.errno not in {
        errno.EADDRNOTAVAIL,
        errno.EAFNOSUPPORT,
        errno.EPROTONOSUPPORT,
    }:
        raise SystemExit(1)
finally:
    for listener in listeners:
        listener.close()
PY
}

runtime_hash() {
  local dotfile
  dotfile="$(cd "$1" && pwd -P)/.devenv"
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$dotfile" | sha256sum | cut -c1-7
  else
    printf '%s' "$dotfile" | shasum -a 256 | cut -c1-7
  fi
}

wait_for_manager_pid() {
  local repo=$1
  # The manager publishes its PID file only after every process passes its
  # readiness probe, so `up` returning a bound port does not yet mean the
  # manager is discoverable. Commands that seed from it must wait for this.
  local pid_file="${XDG_RUNTIME_DIR:-/tmp}/devenv-$(runtime_hash "$repo")/processes/native-manager.pid"
  for _ in $(seq 1 300); do
    if [ -s "$pid_file" ]; then
      return 0
    fi
    sleep 0.1
  done
  echo "Timed out waiting for $pid_file"
  return 1
}

start_repo() {
  local repo=$1
  rm -f "$repo/allocated-port" "$repo/server.pid" "$repo/up.log"
  (cd "$repo" && devenv --no-tui up > up.log 2>&1) &
  echo $! > "$repo/up.pid"
}

start_repo_detached() {
  local repo=$1
  rm -f "$repo/allocated-port" "$repo/server.pid" "$repo/up.log"
  (cd "$repo" && devenv --no-tui up -d > up.log 2>&1)
}

stop_repo() {
  local repo=$1
  if [ -d "$repo" ]; then
    (cd "$repo" && devenv processes down >/dev/null 2>&1) || true
  fi
  if [ -f "$repo/up.pid" ]; then
    kill "$(cat "$repo/up.pid")" 2>/dev/null || true
    wait "$(cat "$repo/up.pid")" 2>/dev/null || true
    rm -f "$repo/up.pid"
  fi
  if [ -f "$repo/server.pid" ]; then
    kill "$(cat "$repo/server.pid")" 2>/dev/null || true
    rm -f "$repo/server.pid"
  fi
}

rm -rf repo1 repo2
make_repo repo1
make_repo repo2

# Warm repo2 while the base port is free. This creates eval-cache entries that
# must be replayed/revalidated when repo2 starts again below.
start_repo_detached repo2 || { cat repo2/up.log; exit 1; }
wait_until 15 test -s "repo2/allocated-port" || { cat repo2/up.log; exit 1; }
repo2_warm_port=$(cat repo2/allocated-port)
if [ "$repo2_warm_port" != "$base_port" ]; then
  echo "Expected warm repo2 run to use free base port $base_port"
  cat repo2/up.log
  exit 1
fi
stop_repo repo2
wait_until 15 tcp_is_gone "$base_port" || { cat repo2/up.log; exit 1; }
wait_until 15 port_is_allocatable "$base_port" || { cat repo2/up.log; exit 1; }

start_repo repo1
wait_until 15 test -s "repo1/allocated-port" || { cat repo1/up.log; exit 1; }
repo1_process_port=$(cat repo1/allocated-port)
wait_until 15 tcp_is_ready "$repo1_process_port" || { cat repo1/up.log; exit 1; }

start_repo repo2
wait_until 15 test -s "repo2/allocated-port" || { cat repo2/up.log; exit 1; }
repo2_process_port=$(cat repo2/allocated-port)
wait_until 15 tcp_is_ready "$repo2_process_port" || { cat repo2/up.log; exit 1; }

echo "base port: $base_port"
echo "repo2 warm port: $repo2_warm_port"
echo "repo1 process port: $repo1_process_port"
echo "repo2 process port: $repo2_process_port"

if [ "$repo1_process_port" != "$base_port" ]; then
  echo "Expected first repo to use free base port $base_port"
  exit 1
fi

if [ "$repo2_process_port" = "$base_port" ]; then
  echo "Expected second repo to dynamically skip occupied base port $base_port"
  cat repo2/up.log
  exit 1
fi

if [ "$repo1_process_port" = "$repo2_process_port" ]; then
  echo "Expected two running repos to have distinct process ports"
  exit 1
fi

# Commands other than `up` must resolve the allocated port too, instead of
# falling back to the base port that repo1 is holding.
wait_for_manager_pid repo2 || { cat repo2/up.log; exit 1; }
repo2_shell_port=$(cd repo2 && devenv --no-tui shell -- printenv ALLOCATED_PORT | tail -n 1 | tr -d '[:space:]')
echo "repo2 shell port: $repo2_shell_port"

if [ "$repo2_shell_port" != "$repo2_process_port" ]; then
  echo "Expected devenv shell in repo2 to report its allocated port $repo2_process_port, got $repo2_shell_port"
  cat repo2/up.log
  exit 1
fi

echo "Two devenv projects with the same base port run concurrently."
