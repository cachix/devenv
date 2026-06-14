#!/usr/bin/env bash

set -xe

FILE="$PWD/.devcontainer/devcontainer.json"

if [ -L "$FILE" ]; then
    exit 1
else
    exit 0
fi