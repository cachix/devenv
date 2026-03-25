#!/usr/bin/env bash
set -ex

mkdir -p ../src/_generated/{languages,services,supported-process-managers}

result=$(devenv-build outputs.devenv-generate-option-docs)

cp -r --no-preserve=all "$result"/* ../src/_generated/
