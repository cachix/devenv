#!/usr/bin/env bash
set -euo pipefail

count_file="$DEVENV_STATE/git-hooks-test-install-count"
hook_path="$(git rev-parse --path-format=absolute --git-path hooks)/pre-commit"
trace_dir="$(mktemp -d)"

# Read the install task's own duration from the JSON activity trace.
# This excludes evaluation and task loading of the nested devenv command.
task_duration_ms() {
  local trace="$1" name="$2"
  jq -n -r --arg name "$name" '
    def ns:
      capture("^(?<date>[^.]+)\\.(?<frac>[0-9]+)Z$")
      | ((.date + "Z" | fromdateiso8601) * 1000000000)
        + ((.frac + "000000000")[0:9] | tonumber);
    [inputs | .fields?.event? // empty | select(type == "object" and .activity_kind == "task")] as $events
    | ($events[] | select(.event == "hierarchy") | .tasks[] | select(.name == $name) | .id) as $id
    | ($events[] | select(.event == "start" and .id == $id) | .timestamp | ns) as $start
    | ($events[] | select(.event == "complete" and .id == $id) | .timestamp | ns) as $end
    | ($end - $start) / 1000000 | floor
  ' "$trace"
}

run_install() {
  local trace="$1"
  shift
  devenv --trace-to "json:file:$trace" tasks run devenv:git-hooks:install --show-output "$@" 2>&1
}

if [ "$(cat "$count_file")" != 1 ]; then
  echo "expected initial hook installation to run once" >&2
  exit 1
fi

output="$(run_install "$trace_dir/warmup.json")"
if [ "$(cat "$count_file")" != 1 ]; then
  echo "expected an unchanged hook installation to be cached" >&2
  exit 1
fi
if ! grep -q 'decision=hit' <<< "$output"; then
  echo "expected the cached installation to log decision=hit" >&2
  echo "$output" >&2
  exit 1
fi

# Measure a second cache hit after the nested command's evaluation cache is warm.
git_count_file="$DEVENV_STATE/git-hooks-test-git-count"
printf '0\n' > "$git_count_file"
export DEVENV_GIT_HOOKS_COUNT_FILE="$git_count_file"
run_install "$trace_dir/hit.json" >/dev/null
unset DEVENV_GIT_HOOKS_COUNT_FILE
if [ "$(cat "$git_count_file")" != 2 ]; then
  echo "expected a cache hit to invoke Git only for hooks-dir resolution and hook hashing" >&2
  exit 1
fi
hit_ms="$(task_duration_ms "$trace_dir/hit.json" devenv:git-hooks:install)"

printf '\n# user modification\n' >> "$hook_path"
output="$(run_install "$trace_dir/stale-hook.json")"
stale_hook_ms="$(task_duration_ms "$trace_dir/stale-hook.json" devenv:git-hooks:install)"
if [ "$(cat "$count_file")" != 2 ]; then
  echo "expected a modified hook to force reinstallation" >&2
  exit 1
fi
if ! grep -q 'decision=stale_hook' <<< "$output"; then
  echo "expected a modified hook to log decision=stale_hook" >&2
  echo "$output" >&2
  exit 1
fi

git config core.hooksPath .custom-hooks
output="$(run_install "$trace_dir/hooks-path.json")"
if [ "$(cat "$count_file")" != 3 ]; then
  echo "expected a changed hooks path to force reinstallation" >&2
  exit 1
fi
if ! grep -q 'decision=miss' <<< "$output"; then
  echo "expected a changed hooks path to log decision=miss" >&2
  echo "$output" >&2
  exit 1
fi
if [ ! -x .custom-hooks/pre-commit ]; then
  echo "expected the hook to be reinstalled at the new repository target" >&2
  exit 1
fi

config_copy="$(mktemp)"
cp -L .pre-commit-config.yaml "$config_copy"
printf '\n# changed generated config\n' >> "$config_copy"
rm .pre-commit-config.yaml
mv "$config_copy" .pre-commit-config.yaml
output="$(run_install "$trace_dir/config.json" --mode single)"
if [ "$(cat "$count_file")" != 4 ]; then
  echo "expected changed generated config content to force reinstallation" >&2
  exit 1
fi
if ! grep -q 'decision=miss' <<< "$output"; then
  echo "expected changed generated config content to log decision=miss" >&2
  echo "$output" >&2
  exit 1
fi

echo "git-hooks install benchmark: hit=${hit_ms}ms stale_hook=${stale_hook_ms}ms"
