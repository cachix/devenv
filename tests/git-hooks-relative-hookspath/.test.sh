#!/usr/bin/env bash
set -euo pipefail

mkdir repro
cd repro

git init -q

cat > devenv.nix <<'EOF'
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

cat > devenv.yaml <<'EOF'
inputs:
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
EOF

git config core.hooksPath .git/hooks

devenv shell -- true

if [ "$(git config --local --get core.hooksPath 2>/dev/null || true)" != ".git/hooks" ]; then
  echo "expected repo-local core.hooksPath=.git/hooks to be preserved"
  git config --show-origin --show-scope --get core.hooksPath || true
  exit 1
fi

if ! test -f .git/hooks/pre-commit; then
  echo "pre-commit hook was not installed in .git/hooks"
  exit 1
fi

printf 'initial\n' > tracked.txt
git add tracked.txt
git -c user.name=test -c user.email=test@example.com commit -q -m init

if ! test -f .hook-ran; then
  echo "pre-commit hook did not run with core.hooksPath=.git/hooks"
  exit 1
fi
