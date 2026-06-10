#!/usr/bin/env bash
set -euo pipefail

# Regression test for https://github.com/cachix/devenv/issues/2924:
# git hooks must install and run when the devenv root is a subdirectory
# of the git repository root.

mkdir -p repro/java
cd repro
git init -q

cat > java/devenv.nix <<'EOF'
{
  git-hooks.hooks.no-op = {
    enable = true;
    name = "No Op";
    pass_filenames = false;
    raw.always_run = true;
    entry = "sh -c 'echo ran >> .hook-ran'";
  };
}
EOF

cat > java/devenv.yaml <<'EOF'
inputs:
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
EOF

(cd java && devenv shell -- true)

hooks_path="$(git rev-parse --path-format=absolute --git-path hooks)"
if ! test -f "$hooks_path/pre-commit"; then
  echo "pre-commit hook was not installed"
  exit 1
fi

printf 'initial\n' > tracked.txt
git add tracked.txt java/devenv.nix java/devenv.yaml
git -c user.name=test -c user.email=test@example.com commit -q -m "trigger hook"

if ! test -f .hook-ran && ! test -f java/.hook-ran; then
  echo "pre-commit hook did not run on commit from the repository root"
  exit 1
fi
