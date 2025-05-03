#!/usr/bin/env bash

set -ex
rm -rf test_proj
gleam --version
gleam new test_proj

# These are currently too flaky to run in CI
# cd test_proj
# gleam test
