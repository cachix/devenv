#!/usr/bin/env bash
set -ex

# Blocks until OpenBao's readiness probe passes, which requires it to be
# initialized, unsealed and active.
wait_for_processes

bao version
bao status

# The devenv token was created during self-initialization.
bao token lookup >/dev/null

# The KV v2 secrets engine is mounted at secret/.
bao kv put secret/devenv hello=world
test "$(bao kv get -field=hello secret/devenv)" = "world"
