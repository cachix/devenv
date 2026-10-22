#!/usr/bin/env bash
set -euo pipefail

# Use a network Git input so revisions are pinned. git+file inputs stay live.
daemon_pid=''
cleanup() {
  if [[ -n "$daemon_pid" ]]; then
    kill "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT
ready=false
for _attempt in {1..10}; do
  port=$((20000 + RANDOM))
  remote_url="git://127.0.0.1:$port/remote?dir=profile"
  git daemon --reuseaddr --export-all --listen=127.0.0.1 --port="$port" \
    --base-path="$PWD" "$PWD/remote" > git-daemon.log 2>&1 &
  daemon_pid=$!
  for _probe in {1..50}; do
    if timeout 2s git ls-remote "${remote_url%\?*}" >/dev/null 2>&1; then
      ready=true
      break
    fi
    kill -0 "$daemon_pid" 2>/dev/null || break
    sleep 0.1
  done
  if "$ready"; then break; fi
  cleanup
  daemon_pid=''
done
if ! "$ready"; then
  cat git-daemon.log >&2
  exit 1
fi
sed "s|git+file://$PWD/remote?dir=profile|$remote_url|" devenv.yaml > devenv.yaml.new
mv devenv.yaml.new devenv.yaml

# Expand environment variables inside the activated shell.
# shellcheck disable=SC2016
devenv shell -- bash -c '
  test "$REMOTE_MODULE" = remote-tool
  test "$REMOTE_NESTED_MODULE" = nested-tool
'
cp devenv.lock before-update.lock

# The updated configuration changes an existing input and adds a transitive
# YAML input. update must compose both before committing the new lock.
printf '%s\n' remote-tool-updated > remote/profile/tool/marker
mkdir -p remote/profile/added/tool
cat > remote/profile/added/devenv.yaml <<'YAML'
inputs:
  added-tool:
    url: path:./tool
    flake: false
YAML
cat > remote/profile/added/devenv.nix <<'NIX'
{ inputs, ... }:
{
  env.REMOTE_ADDED = builtins.replaceStrings [ "\n" ] [ "" ]
    (builtins.readFile (inputs.added-tool + /marker));
}
NIX
printf '%s\n' added-tool > remote/profile/added/tool/marker
printf '%s\n' '  - ./added' >> remote/profile/devenv.yaml
git -C remote add .
git -C remote -c user.email=test@devenv -c user.name=devenv-test \
  -c commit.gpgsign=false commit -q -m 'update remote inputs'

devenv update shared
if cmp -s before-update.lock devenv.lock; then
  echo 'update did not change the remote input lock' >&2
  exit 1
fi
cp devenv.lock after-update.lock
# Expand environment variables inside the activated shell.
# shellcheck disable=SC2016
devenv shell -- bash -c '
  test "$REMOTE_MODULE" = remote-tool-updated
  test "$REMOTE_NESTED_MODULE" = nested-tool
  test "$REMOTE_ADDED" = added-tool
'
cmp after-update.lock devenv.lock
