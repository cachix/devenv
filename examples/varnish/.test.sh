#!/usr/bin/env bash
set -ex

wait_for_port 6081

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
