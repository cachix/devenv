#!/usr/bin/env bash
set -e

# First run: seed both files from their templates
devenv shell true

# Both files are real writable files, not symlinks into the store
test -f template.txt && test ! -L template.txt
test -f managed.txt && test ! -L managed.txt
test -w template.txt
test -w managed.txt

# Simulate the user editing both files
echo "user edit" > template.txt
echo "user edit" > managed.txt

# Second run re-applies the files. enterTest (in devenv.nix) asserts the
# resulting state when `devenv test` runs after this script.
devenv shell true
