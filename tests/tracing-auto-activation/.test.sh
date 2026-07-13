#!/usr/bin/env bash
set -euo pipefail

trace_dir=$(mktemp -d)

assert_trace_span() {
  local file=$1
  local caller=$2
  local command=$3

  if ! jq -e --arg caller "$caller" --arg command "$command" '
    select(
      .span.name == "devenv"
      and .span["devenv.caller"] == $caller
      and .span["devenv.command"] == $command
    )
  ' "$file" >/dev/null; then
    echo "expected trace $file to contain a devenv span for caller=$caller command=$command" >&2
    cat "$file" >&2
    exit 1
  fi
}

direnv_trace="$trace_dir/direnv.json"
_DEVENV_CALLER=direnv devenv --verbose --trace-to "json:file:$direnv_trace" direnv-export >/dev/null
assert_trace_span "$direnv_trace" direnv direnv-export

hook_trace="$trace_dir/hook.json"
_DEVENV_CALLER=hook _DEVENV_HOOK_DIR="$PWD" devenv --verbose --trace-to "json:file:$hook_trace" \
  shell -- sh -c 'test -z "${_DEVENV_CALLER+x}"'
assert_trace_span "$hook_trace" hook shell

command_trace="$trace_dir/command.json"
devenv --verbose --trace-to "json:file:$command_trace" info >/dev/null
assert_trace_span "$command_trace" cli info

# `_DEVENV_HOOK_DIR` lives for the duration of a hook-spawned shell. It must
# not classify later explicit commands as hook calls.
nested_trace="$trace_dir/nested.json"
_DEVENV_HOOK_DIR="$PWD" devenv --verbose --trace-to "json:file:$nested_trace" shell -- true
assert_trace_span "$nested_trace" cli shell

devenv direnvrc | grep -q '_DEVENV_CALLER=direnv'
for shell in bash zsh fish nu; do
  devenv hook "$shell" | grep -q '_DEVENV_CALLER'
done
