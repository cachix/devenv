#!/usr/bin/env bash

set -euo pipefail

base=$(devenv eval profilePriorityTest.package.pname)
echo "$base" | grep -Eq '"profilePriorityTest.package.pname"[[:space:]]*:[[:space:]]*"hello"'

profile=$(devenv --profile package-override eval profilePriorityTest.package.pname)
echo "$profile" | grep -Eq '"profilePriorityTest.package.pname"[[:space:]]*:[[:space:]]*"curl"'
