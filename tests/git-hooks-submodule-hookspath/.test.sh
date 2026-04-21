#!/usr/bin/env bash
set -euo pipefail

mkdir repro
cd repro

git init -q submodule-src
cd submodule-src
printf 'initial\n' > tracked.txt
git add tracked.txt
git -c user.name=test -c user.email=test@example.com commit -q -m init
cd ..

git init -q parent
git -C parent -c protocol.file.allow=always submodule add -q ../submodule-src vendor/sub

submodule_path="$PWD/parent/vendor/sub"

cat > "$submodule_path/devenv.nix" <<'EOF'
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

cat > "$submodule_path/devenv.yaml" <<'EOF'
inputs:
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
EOF

expected_hooks_path="$(git -C "$submodule_path" rev-parse --path-format=absolute --git-path hooks)"
git -C "$submodule_path" config core.hooksPath "$expected_hooks_path"

cd "$submodule_path"
devenv shell -- true

if [ "$(git config --get core.hooksPath 2>/dev/null || true)" != "$expected_hooks_path" ]; then
  echo "expected submodule core.hooksPath to be preserved"
  git config --show-origin --show-scope --get core.hooksPath || true
  exit 1
fi

devenv shell -- true

if ! test -f "$(git rev-parse --git-dir)/hooks/pre-commit"; then
  echo "pre-commit hook was not installed in the submodule hooks dir"
  exit 1
fi

if [ "$(git config --get core.hooksPath 2>/dev/null || true)" != "$expected_hooks_path" ]; then
  echo "expected submodule core.hooksPath to remain preserved after repeated install"
  git config --show-origin --show-scope --get core.hooksPath || true
  exit 1
fi

printf 'change\n' >> tracked.txt
git add tracked.txt
git -c user.name=test -c user.email=test@example.com commit -q -m "trigger hook"

if ! test -f .hook-ran; then
  echo "pre-commit hook did not run in the submodule"
  exit 1
fi
