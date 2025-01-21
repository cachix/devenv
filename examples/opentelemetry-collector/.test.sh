#!/usr/bin/env bash
set -ex

endpoint="http://localhost:13133/"

timeout 20 bash -c "until curl $endpoint 2>/dev/null; do sleep 0.5; done"
curl -s "$endpoint" | grep "Server"
