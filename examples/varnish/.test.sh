#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

devenv_stop() {
    pkill -P "$DEVENV_PID"
}

trap devenv_stop EXIT

timeout 20 bash -c 'until echo > /dev/tcp/localhost/6081; do sleep 0.5; done'

caddy=$(curl http://localhost:8001)
varnish=$(curl http://localhost:6081)

if [[ "$caddy" == "$varnish" ]]; then
  echo "Everything running";
else
  echo "Caddy response does not match Varnish";
  echo "Caddy response: ${caddy}"
  echo "Varnish response: ${varnish}"
  exit 1
fi
