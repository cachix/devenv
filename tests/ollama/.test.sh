#!/usr/bin/env bash
set -ex

wait_for_processes

curl --fail --silent "http://$OLLAMA_HOST/api/version"
curl --fail --silent "$OLLAMA_BASE_URL/models"
ollama list

test -d "$DEVENV_STATE/ollama/models"
