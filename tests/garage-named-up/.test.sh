#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  devenv processes down >/dev/null 2>&1 || true
}
trap cleanup EXIT

# A named start selects only Garage as a process root. The configure task must
# still run so Garage can become ready and create the requested bucket.
devenv up -d garage
devenv processes wait --timeout 120
devenv shell -- bash -c 'garage -c "$GARAGE_CONFIG_FILE" bucket info named-up-bucket'
