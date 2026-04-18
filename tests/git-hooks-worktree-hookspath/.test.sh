#!/usr/bin/env bash
set -euo pipefail

mkdir repro
cd repro

git init -q --bare bare.git
git clone -q bare.git seed
cd seed
printf 'initial\n' > tracked.txt
git add tracked.txt
git -c user.name=test -c user.email=test@example.com commit -q -m init
git push -q origin HEAD:main
cd ..

git --git-dir=bare.git worktree add -q wt main

cat > wt/devenv.nix <<'EOF'
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

cat > wt/devenv.yaml <<'EOF'
inputs:
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
EOF

cd wt
devenv shell -- true

if [ -n "$(git config --get core.hooksPath 2>/dev/null || true)" ]; then
  echo "expected worktree core.hooksPath to remain unset"
  git config --show-origin --show-scope --get core.hooksPath || true
  exit 1
fi

devenv shell -- true

if [ -n "$(git config --get core.hooksPath 2>/dev/null || true)" ]; then
  echo "expected worktree core.hooksPath to remain unset after repeated install"
  git config --show-origin --show-scope --get core.hooksPath || true
  exit 1
fi

if ! test -f "$(git rev-parse --git-common-dir)/hooks/pre-commit"; then
  echo "pre-commit hook was not installed in the shared worktree hooks dir"
  exit 1
fi

printf 'change\n' >> tracked.txt
git add tracked.txt
git -c user.name=test -c user.email=test@example.com commit -q -m "trigger hook"

if ! test -f .hook-ran; then
  echo "pre-commit hook did not run in the linked worktree"
  exit 1
fi
