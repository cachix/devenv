#!/usr/bin/env bash
set -euo pipefail

mkdir -p mock-bin
export TRANSACTION_LOG="$PWD/commands.log"
export TRANSACTION_ID="$PWD/request-id"
export REAL_NIX
REAL_NIX=$(command -v nix)
cat >mock-bin/ssh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
remote="${!#}"
if [[ "$remote" == *devenv-machine-facts-v1* ]]; then
  printf '%s\nnull\n' "$TRANSACTION_SYSTEM"
  exit 0
fi
if [[ "$remote" == *devenv-machine-plan-v1* ]]; then
  printf 'devenv-machine-plan-v1\n%s\n%s\n%s\n' "$TRANSACTION_SYSTEM" "$TRANSACTION_SYSTEM" "$TRANSACTION_SYSTEM"
  exit 0
fi
printf '%s\n' "$remote" >>"$TRANSACTION_LOG"
request=" (start|rollback) '([A-Za-z0-9-]+)'"
if [[ "$remote" =~ $request ]]; then
  printf '%s' "${BASH_REMATCH[2]}" >"$TRANSACTION_ID"
  rm -f "$TRANSACTION_ID.confirmed"
  if [[ "${TRANSACTION_LOST:-}" == 1 ]]; then
    exit 255
  fi
  printf '{}\n'
elif [[ "$remote" == *" confirm "* ]]; then
  [[ "$remote" == *"'$(cat "$TRANSACTION_ID")'"* ]]
  touch "$TRANSACTION_ID.confirmed"
  if [[ "${TRANSACTION_LOST_CONFIRM:-}" == 1 ]]; then exit 255; fi
  printf '{}\n'
elif [[ "$remote" == *status* ]]; then
  if [[ "${TRANSACTION_CONFIRM:-}" == 1 && ! -e "$TRANSACTION_ID.confirmed" ]]; then
    printf '{"version":1,"id":"%s","outcome":"pending","phase":"awaiting-confirmation"}\n' "$(cat "$TRANSACTION_ID")"
    exit 0
  fi
  printf '{"version":1,"id":"%s","outcome":"%s","error":"fixture activation failed"}\n' \
    "$(cat "$TRANSACTION_ID")" "${TRANSACTION_OUTCOME:-succeeded}"
else
  echo "unexpected remote command: $remote" >&2
  exit 98
fi
EOF
cat >mock-bin/nix <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == copy ]]; then
  printf 'copy %s\n' "${!#}" >>"$TRANSACTION_LOG"
else
  exec "$REAL_NIX" "$@"
fi
EOF
chmod +x mock-bin/*
export PATH="$PWD/mock-bin:$PATH"

export TRANSACTION_SYSTEM
TRANSACTION_SYSTEM=$(devenv build machines.server.build.nixos | jq -er '."machines.server.build.nixos"')

TRANSACTION_CONFIRM=1 devenv machines deploy server --yes
grep -q ' confirm ' "$TRANSACTION_LOG"
grep -q 'transaction-test-system' "$TRANSACTION_LOG"
grep -q 'transaction-test-executor' "$TRANSACTION_LOG"

# Status and rollback work without building or transferring another output.
: >"$TRANSACTION_LOG"
devenv machines status server | jq -e '.server.outcome == "succeeded"'
devenv machines rollback server
if grep -q '^copy ' "$TRANSACTION_LOG"; then
  echo "status or rollback copied a closure"
  exit 1
fi

for outcome in failed unknown rolled-back rollback-failed; do
  if TRANSACTION_OUTCOME="$outcome" devenv machines deploy server --yes >"$outcome.log" 2>&1; then
    echo "transaction with $outcome outcome reported success"
    exit 1
  fi
  grep -q 'Transaction' "$outcome.log"
done

if TRANSACTION_LOST=1 devenv machines deploy server --yes >lost.log 2>&1; then
  echo "lost submission response reported success"
  exit 1
fi
grep -q 'may have started' lost.log

if TRANSACTION_CONFIRM=1 TRANSACTION_LOST_CONFIRM=1 devenv machines deploy server --yes >lost-confirm.log 2>&1; then
  echo "lost confirmation response reported success"
  exit 1
fi
grep -q 'Could not acknowledge confirmation' lost-confirm.log

# The unreleased direct NixOS bypass has been removed.
: > "$TRANSACTION_LOG"
if devenv machines deploy server --legacy > removed-flag.log 2>&1; then exit 1; fi
grep -q "unexpected argument '--legacy'" removed-flag.log
test ! -s "$TRANSACTION_LOG"
