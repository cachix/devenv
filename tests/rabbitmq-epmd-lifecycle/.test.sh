#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

cleanup() {
  devenv processes down >/dev/null 2>&1 || true
}
trap cleanup EXIT

epmd_listening() {
  (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

epmd_stopped() {
  ! epmd_listening "$1"
}

port_file=.devenv/state/epmd-port
for attempt in 1 2; do
  rm -f "$port_file"
  devenv up -d
  devenv processes wait
  wait_until 30 test -s "$port_file"
  read -r port < "$port_file"
  epmd_listening "$port"
  devenv processes down
  wait_until 15 epmd_stopped "$port"
done
