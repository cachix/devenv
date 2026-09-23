#!/usr/bin/env bash
# Exercise the packaged CLI, including its native VT library, on a real PTY.
# Usage: smoke.sh DEVENV PTY_DRIVER MODULES OUTPUT_DIRECTORY
# Pass absolute paths; MODULES is this checkout's src/modules directory.
set -euo pipefail

export DEVENV_SMOKE_BIN="$1"
driver="$2"
modules="$3"
mkdir -p "$4"
output=$(cd "$4" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Isolate devenv preferences/trust and opt out of agent/CI detection.
export XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export DEVENV_HOME="$work/devenv-home"
export PATH="$(dirname "$DEVENV_SMOKE_BIN"):$PATH"
export DEVENV_NO_AI_AGENT=1 TERM=xterm-256color
unset CI DEVENV_TUI DEVENV_TRACE_TO DEVENV_SHELL_TYPE _DEVENV_SHELL_HINT _DEVENV_HOOK_DIR DEVENV_ROOT
"$DEVENV_SMOKE_BIN" --version > "$output/version.txt"
mkdir -p "$work/project"
cd "$work/project"
# Reuse the repository's pinned nixpkgs rather than resolve a moving branch.
cp "$modules/../../devenv.lock" devenv.lock
cat > devenv.nix <<'EOF'
{ ... }: {
  env.SMOKE_VALUE = "7319";
  enterShell = ''echo SMOKE_READY'';
}
EOF
cat > devenv.yaml <<EOF
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  devenv:
    url: path:$modules
EOF

for size in 80x24 0x0 0x24 80x0; do
  status=0
  "$driver" pty --size "$size" --step-timeout 180 \
    "$output/shell-$size.typescript" 'exec "$DEVENV_SMOKE_BIN" --shell bash shell' <<'EOF' || status=$?
expect:SMOKE_READY
send:printf 'SMOKE_RESULT=%s\\n' "$((SMOKE_VALUE + 1))"\n
expect:SMOKE_RESULT=7320
resize:100x30
send:printf 'RESIZED_RESULT=%s\\n' "$((SMOKE_VALUE + 2))"\n
expect:RESIZED_RESULT=7321
send:exit 42\n
EOF
  if [ "$status" -ne 42 ]; then
    tail -c 8000 "$output/shell-$size.typescript"
    echo "Interactive shell at $size: expected exit 42, got $status" >&2
    exit 1
  fi
  echo "Passed interactive shell at $size"
done

# Activate through the real bash prompt hook after changing directories.
# Use a marker from the parent Bash's PROMPT_COMMAND so the exit check cannot
# match the nested devenv shell's prompt, which contains the same PS1 text.
"$DEVENV_SMOKE_BIN" allow
cd "$work"
"$driver" pty --step-timeout 180 "$output/hook.typescript" \
  'export PS1="SMOKE_OUTER> "; exec bash --noprofile --norc -i' <<'EOF'
expect:SMOKE_OUTER>
send:PROMPT_COMMAND='echo SMOKE_PARENT_READY'; eval "$("$DEVENV_SMOKE_BIN" hook bash)"\n
expect:SMOKE_OUTER>
send:cd project\n
expect:SMOKE_READY
send:printf 'HOOK_RESULT=%s\\n' "$((SMOKE_VALUE + 1))"\n
expect:HOOK_RESULT=7320
send:exit\n
expect:SMOKE_PARENT_READY
send:exit\n
EOF
echo "Passed bash hook activation"
