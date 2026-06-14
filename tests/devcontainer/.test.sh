#!/usr/bin/env bash

set -xe

FILE="$PWD/.devcontainer/devcontainer.json"

if [ ! -e "$FILE" ]; then
    echo "File does not exist"
    exit 1
elif [ -L "$FILE" ]; then
    echo "File is a symlink"
    exit 1
else
    exit 0
fi