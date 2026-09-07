#!/usr/bin/env bash
set -euxo pipefail

wait_for_port 80

env | grep UNIX_SOCKET
env | grep TRUST_PROXY
env | grep APP_NAME

curl -sf "http://localhost/setup"
