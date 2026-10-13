#!/usr/bin/env bash
set -euo pipefail

fail() { echo "FAIL: $*" >&2; exit 1; }

space_dir="$PWD/Some App/bin"
tail_dir="$PWD/After Space/bin"
mkdir -p "$space_dir" "$tail_dir" "$PWD/zsh-config"
export PATH="$space_dir:$PATH:$tail_dir"
export ZDOTDIR="$PWD/zsh-config"

# Piped stdin exercises prepare_shell's env-script path. zsh is provided by
# the fixture so a missing shell cannot be mistaken for a successful test.
cat > shell-commands.zsh <<'COMMANDS'
printf 'PATH_CHECK=%s\n' "$PATH"
exit
COMMANDS
if ! devenv --no-reload --shell zsh shell <shell-commands.zsh \
  >devenv-shell.stdout 2>devenv-shell.stderr; then
  cat devenv-shell.stdout devenv-shell.stderr >&2
  fail "zsh shell failed"
fi
if grep -q "not a valid identifier" devenv-shell.stdout devenv-shell.stderr; then
  cat devenv-shell.stdout devenv-shell.stderr >&2
  fail "PATH exports were parsed without their quotes"
fi

check_path() {
  local value="$1"
  for directory in "$space_dir" "$tail_dir" "$PWD/.devenv/state/venv/bin"; do
    case ":$value:" in
      *":$directory:"*) ;;
      *) fail "missing PATH entry '$directory' in: $value" ;;
    esac
  done
}

shell_path=$(sed -n 's/^PATH_CHECK=//p' devenv-shell.stdout)
check_path "$shell_path"
[ -f .devenv/shell-env.sh ] || fail "env script was not written"
if grep -E 'eval "\$\{shellHook:-\}"export' .devenv/shell-env.sh; then
  fail "task exports were glued onto the shell hook"
fi
check_path "$(devenv shell -- printenv PATH)"
