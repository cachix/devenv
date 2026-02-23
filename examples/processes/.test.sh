#!/usr/bin/env bash
set -euxo pipefail

wait_for_processes

PORT=$(devenv processes port server http)
curl -vf "http://127.0.0.1:$PORT/"
