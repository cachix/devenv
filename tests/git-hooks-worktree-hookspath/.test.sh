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

expected_hooks_path="$(git rev-parse --path-format=absolute --git-common-dir)/hooks"
if [ "$(git config --get core.hooksPath 2>/dev/null || true)" != "$expected_hooks_path" ]; then
  echo "expected worktree hooksPath to be restored after install"
  git config --show-origin --show-scope --get core.hooksPath || true
  exit 1
fi

devenv shell -- true

printf 'change\n' >> tracked.txt
git add tracked.txt
git -c user.name=test -c user.email=test@example.com commit -q -m "trigger hook"

if ! test -f .hook-ran; then
  echo "pre-commit hook did not run in the linked worktree"
  exit 1
fi
