#!/usr/bin/env bash
set -euo pipefail

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT

for name in 01-failed-patch 02-next-test; do
  mkdir "$fixtures/$name"
  echo '{ ... }: { }' > "$fixtures/$name/devenv.nix"
  printf 'use_shell: false\ngit_init: false\n' > "$fixtures/$name/.test-config.yml"
done

echo 'exit 23' > "$fixtures/01-failed-patch/.patch.sh"
echo 'echo TEST_SHOULD_NOT_RUN' > "$fixtures/01-failed-patch/.test.sh"
echo 'echo NEXT_TEST_RAN' > "$fixtures/02-next-test/.test.sh"

if devenv-run-tests run "$fixtures" > "$fixtures/output.log" 2>&1; then
  cat "$fixtures/output.log"
  echo "Expected a failing patch script to fail its test"
  exit 1
fi

cat "$fixtures/output.log"
grep -q 'Patch script failed. Status code: 23' "$fixtures/output.log"
grep -q 'NEXT_TEST_RAN' "$fixtures/output.log"
grep -q 'Ran 2 tests, 1 failed, 0 skipped' "$fixtures/output.log"
if grep -q 'TEST_SHOULD_NOT_RUN' "$fixtures/output.log"; then
  echo "The test ran after its patch script failed"
  exit 1
fi
