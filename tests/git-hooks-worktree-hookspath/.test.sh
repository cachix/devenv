#!/usr/bin/env bash
set -euo pipefail

check_worktrees() (
  local scenario="$1" devenv_dir="$2" config_path="$3" package="$4"
  mkdir "$scenario"
  cd "$scenario"

  git init -q --bare bare.git
  git clone -q bare.git seed
  cd seed
  printf 'initial\n' > tracked.txt
  git add tracked.txt
  git -c user.name=test -c user.email=test@example.com commit -q -m init
  git push -q origin HEAD:main
  cd ..

  local worktree
  for worktree in first second; do
    git --git-dir=bare.git worktree add -q -b "$worktree" "$worktree" main
    mkdir -p "$worktree/$devenv_dir" "$worktree/nested"
    cat > "$worktree/$devenv_dir/devenv.nix" <<NIX
{ pkgs, ... }:
{
  git-hooks.package = pkgs.$package;
  git-hooks.configPath = "$config_path";
  git-hooks.hooks.no-op = {
    enable = true;
    name = "No Op";
    pass_filenames = false;
    raw.always_run = true;
    entry = "sh -c 'echo $worktree >> .hook-ran'";
  };
}
NIX

    cat > "$worktree/$devenv_dir/devenv.yaml" <<'YAML'
inputs:
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
YAML

    # Installing without -c would discover and run this unrelated config.
    cat > "$worktree/nested/.pre-commit-config.yaml" <<'YAML'
repos:
  - repo: local
    hooks:
      - id: unexpected-nested-hook
        name: This config must not run
        language: system
        entry: 'false'
        always_run: true
        pass_filenames: false
YAML
  done

  local installed_from
  for installed_from in first second first; do
    (cd "$installed_from/$devenv_dir" && devenv shell -- true)

    # A repeated shell entry must also preserve the shared hook.
    (cd "$installed_from/$devenv_dir" && devenv shell -- true)

    for worktree in first second; do
      (
        cd "$worktree"
        if [ -n "$(git config --get core.hooksPath 2>/dev/null || true)" ]; then
          echo "expected worktree core.hooksPath to remain unset"
          git config --show-origin --show-scope --get core.hooksPath || true
          exit 1
        fi

        hook="$(git rev-parse --git-common-dir)/hooks/pre-commit"
        if [ ! -f "$hook" ]; then
          echo "pre-commit hook was not installed in the shared worktree hooks dir"
          exit 1
        fi
        if grep -qF "$PWD/" "$hook"; then
          echo "pre-commit hook pins a worktree path"
          exit 1
        fi

        # The second worktree's config is generated on its first shell entry.
        if [ ! -f "$devenv_dir/$config_path" ]; then
          exit 0
        fi
        rm -f .hook-ran
        printf 'change\n' >> tracked.txt
        git add tracked.txt
        git -c user.name=test -c user.email=test@example.com commit -q -m "trigger hook"
        if [ "$(cat .hook-ran)" != "$worktree" ]; then
          echo "commit in $worktree ran another worktree's config"
          exit 1
        fi
      )
    done
  done
)

# Cover both the default config and a custom path below a devenv subdirectory.
check_worktrees default . .pre-commit-config.yaml prek
check_worktrees subdirectory 'project dir' 'custom config.yaml' prek
check_worktrees pre-commit 'project dir' 'custom config.yaml' pre-commit
