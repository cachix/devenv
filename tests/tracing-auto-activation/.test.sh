#!/usr/bin/env bash
set -euo pipefail

trace_dir=$(mktemp -d)

assert_trace_contains() {
  local file=$1
  local pattern=$2

  if ! grep -q "$pattern" "$file"; then
    echo "expected trace $file to contain: $pattern" >&2
    cat "$file" >&2
    exit 1
  fi
}

direnv_trace="$trace_dir/direnv.json"
devenv --verbose --trace-to "json:file:$direnv_trace" direnv-export >/dev/null
assert_trace_contains "$direnv_trace" "devenv.auto-activation"
assert_trace_contains "$direnv_trace" "auto_activation"
assert_trace_contains "$direnv_trace" "direnv"
assert_trace_contains "$direnv_trace" "direnv-export"

hook_trace="$trace_dir/hook.json"
_DEVENV_HOOK_DIR="$PWD" devenv --verbose --trace-to "json:file:$hook_trace" shell -- true
assert_trace_contains "$hook_trace" "devenv.auto-activation"
assert_trace_contains "$hook_trace" "auto_activation"
assert_trace_contains "$hook_trace" "hook"
assert_trace_contains "$hook_trace" "shell"

command_trace="$trace_dir/command.json"
devenv --verbose --trace-to "json:file:$command_trace" info >/dev/null
assert_trace_contains "$command_trace" '"message":"devenv"'
assert_trace_contains "$command_trace" "command"
assert_trace_contains "$command_trace" "cli"
assert_trace_contains "$command_trace" "info"
