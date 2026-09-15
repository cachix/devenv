#!/usr/bin/env bash
set -e

# Set up the destinations enterTest (in devenv.nix) asserts on. The first run creates every
# file; the state left here is what the run inside `devenv test` has to replace.
devenv shell true

# A user edit: the copy is renamed over an existing regular file.
echo "user edit" > managed.yaml

# A symlink: the copy replaces the link itself instead of writing through it.
echo "untouched" > decoy.yaml
rm linked.yaml
ln -s decoy.yaml linked.yaml

# A symlink to a directory: `mv` resolves such a link and would drop the copy inside the
# directory, so the link has to be unlinked before the rename.
mkdir -p decoydir
rm dirlinked.yaml
ln -s decoydir dirlinked.yaml
