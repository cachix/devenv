#!/usr/bin/env bash
set -euxo pipefail

wait_for_processes

curl -vf "http://127.0.0.1:$SERVER_HTTP_PORT/"
