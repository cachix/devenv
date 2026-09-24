#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  for repo in repo2 repo1; do
    if [ -d "$repo" ]; then
      (cd "$repo" && devenv processes down >/dev/null 2>&1) || true
    fi
  done
}
trap cleanup EXIT

base_port=$(python3 - <<'PY'
import socket

for _ in range(1000):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    if port < 65436:
        print(port)
        break
else:
    raise SystemExit("could not find a base port")
PY
)

make_repo() {
  local repo=$1
  mkdir -p "$repo"
  cat > "$repo/devenv.nix" <<'NIX'
{ ... }: {
  services.postgres = {
    enable = true;
    port = __BASE_PORT__;
    listen_addresses = "127.0.0.1";
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

make_repo repo1
make_repo repo2

(cd repo1 && devenv --no-tui up -d > up.log 2>&1) || { cat repo1/up.log; exit 1; }
(cd repo2 && devenv --no-tui up -d > up.log 2>&1) || { cat repo2/up.log; exit 1; }

repo1_status=$(cd repo1 && devenv --no-tui processes list)
repo2_status=$(cd repo2 && devenv --no-tui processes list)
repo1_port=$(printf '%s\n' "$repo1_status" | sed -nE 's/.*ports: main:([0-9]+).*/\1/p')
repo2_port=$(printf '%s\n' "$repo2_status" | sed -nE 's/.*ports: main:([0-9]+).*/\1/p')
if [ -z "$repo1_port" ] || [ -z "$repo2_port" ] || [ "$repo1_port" = "$repo2_port" ]; then
  echo "Expected two running PostgreSQL processes with distinct allocated ports"
  printf 'repo1: %s\nrepo2: %s\n' "$repo1_status" "$repo2_status"
  exit 1
fi

repo2_shell_port=$(cd repo2 && devenv --no-tui shell -- printenv PGPORT | tail -n 1 | tr -d '[:space:]')
repo2_data_dir=$(cd repo2 && devenv --no-tui shell -- psql -d postgres -tAc 'show data_directory' | tail -n 1)
expected_data_dir="$(cd repo2 && pwd -P)/.devenv/state/postgres"

if [ "$repo2_shell_port" != "$repo2_port" ]; then
  echo "Expected repo2 PGPORT $repo2_port, got $repo2_shell_port"
  exit 1
fi

if [ "$repo2_data_dir" != "$expected_data_dir" ]; then
  echo "Expected repo2 shell to connect to $expected_data_dir, got $repo2_data_dir"
  exit 1
fi

echo "PostgreSQL shell port and data directory match repo2's running process."
