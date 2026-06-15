#!/usr/bin/env bash

set -xe

FILE="$PWD/.devcontainer/devcontainer.json"

FILE_CONTENT_BEFORE_DEVENV=$(cat "$FILE")
devenv shell --command exit

BACKUP_DIR=$(find "$PWD" -maxdepth 2 -name "devcontainer.json" \
  ! -path "$PWD/.devcontainer/devcontainer.json" \
  -exec dirname {} \; | head -1)

if [ -z "$BACKUP_DIR" ]; then
  echo "No backup directory found; the pre-existing devcontainer was not preserved"
  exit 1
fi

MOVED_FILE_CONTENT=$(cat $BACKUP_DIR/devcontainer.json)

if [ "$FILE_CONTENT_BEFORE_DEVENV" != "$MOVED_FILE_CONTENT" ]; then
    echo "The content of the pre-existing devcontainer changed"
    exit 1
fi

FILE_CONTENT_AFTER_DEVENV=$(cat "$FILE")

sed -i 's/DEVCONTAINER_IMAGE_PLACEHOLDER/ghcr.io\/cachix\/devenv\/devcontainer:latest/' devenv.nix
devenv shell --command exit

if [ "$MOVED_FILE_CONTENT" != $(cat $PWD/.devcontainer-old/devcontainer.json) ]; then
    echo "The preserved file changed, but devcontainer was managed by devenv"
    exit 1
fi

if [ "$FILE_CONTENT_AFTER_DEVENV" == $(cat "$FILE") ]; then
    echo "Devcontainer file didn't change after changing configuration"
    exit 1
fi

if [ -L "$FILE" ]; then
    echo "Devcontainer file is a symlink"
    exit 1
fi

exit 0