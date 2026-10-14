#!/usr/bin/env bash

set -ex

# node_modules/.bin must be on PATH as an absolute path, so installed binaries
# keep working after leaving the directory the shell started in.
case ":$PATH:" in
*":$DEVENV_ROOT/directory/node_modules/.bin:"*) ;;
*)
  echo "node_modules/.bin is not on PATH as an absolute path: $PATH"
  exit 1
  ;;
esac

cd directory
node -e "require('axios')"
