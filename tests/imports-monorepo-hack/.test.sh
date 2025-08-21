#/usr/bin/env bash

set -euo pipefail

pushd subdir
devenv test
popd
