#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  devenv processes down >/dev/null 2>&1 || true
}
trap cleanup EXIT

# A named start runs in before mode. It must still select MySQL's configure
# task and wait for it to create the database and user.
devenv up -d mysql
devenv processes wait --timeout 120
devenv shell -- bash -c 'mysql -h 127.0.0.1 -P "$MYSQL_TCP_PORT" -u named_up -pnamed_up named_up -e "SELECT 1"'
