#!/usr/bin/env bash
set -xe
set -o pipefail

# devenv canonicalizes DEVENV_ROOT (resolving symlinks like macOS
# `/var -> /private/var`), so compare against the physical path (`pwd -P`),
# not the logical one, or the exact-line match below fails on macOS.
proj_root="$(pwd -P)"
mkdir -p sub/nested

# Discovery from one level deep
(cd sub && devenv print-paths | grep -Fxq "DEVENV_ROOT=\"$proj_root\"")

# Discovery from two levels deep
(cd sub/nested && devenv print-paths | grep -Fxq "DEVENV_ROOT=\"$proj_root\"")

# `devenv shell -- <cmd>` runs from the directory you invoked it in, not the
# discovered project root, so relative paths resolve where the user expects.
(cd sub && echo 'echo ran-in-sub' > rel.sh
 devenv shell -- bash rel.sh | grep -Fq ran-in-sub)

# `inputs add` from a subdir edits the enclosing project's devenv.yaml, not a
# stray one in the subdir.
(cd sub/nested && devenv inputs add subdir-discovery-input github:NixOS/nixpkgs)
grep -Fq "subdir-discovery-input" "$proj_root/devenv.yaml"
test ! -e sub/devenv.yaml
test ! -e sub/nested/devenv.yaml

# Negative case: outside any project, original error preserved
outside="$(mktemp -d)"
trap 'rm -rf "$outside"' EXIT
output="$(cd "$outside" && devenv print-paths 2>&1 || true)"
if echo "$output" | grep -q "devenv.nix does not exist"; then
  echo "✓ negative case: error preserved outside any project"
else
  echo "expected 'devenv.nix does not exist' outside any project, got:"
  echo "$output"
  exit 1
fi
