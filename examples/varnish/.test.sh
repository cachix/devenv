#!/usr/bin/env bash
set -ex

wait_for_port $VARNISH_PORT

caddy=$(curl http://localhost:8001)
varnish=$(curl http://localhost:$VARNISH_PORT)

if [[ "$caddy" == "$varnish" ]]; then
  echo "Everything running";
else
  echo "Caddy response does not match Varnish";
  echo "Caddy response: ${caddy}"
  echo "Varnish response: ${varnish}"
  exit 1
fi
