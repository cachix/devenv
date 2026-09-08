#!/usr/bin/env bash
set -euo pipefail

fail() { echo "FAIL: $*" >&2; exit 1; }

space_dir="$PWD/Some App/bin"
mkdir -p "$space_dir"
export PATH="$space_dir:$PATH"

# Piped stdin is not a TTY, so this uses prepare_shell (not the PTY reload
# path). `--shell zsh` writes `.devenv/shell-env.sh` — the assembly that glued
# task PATH exports onto `eval "${shellHook:-}"`. zsh itself does not need to
# start: the env script is sourced by bash before exec.
set +e
echo exit | devenv --no-reload --shell zsh shell \
  >devenv-shell.stdout 2>devenv-shell.stderr
set -e

if grep -q "not a valid identifier" devenv-shell.stdout devenv-shell.stderr; then
  echo "stdout:" >&2
  cat devenv-shell.stdout >&2
  echo "stderr:" >&2
  cat devenv-shell.stderr >&2
  fail "devenv shell reported 'not a valid identifier' (PATH word-split at a space)"
fi

env_script=.devenv/shell-env.sh
[ -f "$env_script" ] || fail "expected $env_script to be written for --shell zsh"

if grep -E 'eval "\$\{shellHook:-\}"export' "$env_script"; then
  fail "task exports were glued onto the eval \"\${shellHook:-}\" line"
fi

if ! grep -E "^export PATH=.*Some App" "$env_script"; then
  fail "shell-env.sh PATH export is missing the space-containing directory"
fi

# Non-interactive bash path (bash_init_script + task exports) must also keep it.
cmd_path=$(devenv shell -- printenv PATH)
case ":$cmd_path:" in
  *":$space_dir:"*) ;;
  *) fail "devenv shell -- truncated PATH; missing '$space_dir' in: $cmd_path" ;;
esac

echo "OK: PATH with spaces survives venv/task exports"
