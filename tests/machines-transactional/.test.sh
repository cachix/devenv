#!/usr/bin/env bash
set -euo pipefail

mkdir -p mock-bin
export TRANSACTION_LOG="$PWD/commands.log"
export TRANSACTION_ID="$PWD/request-id"
export TRANSACTION_STATUS_COUNT="$PWD/status-count"
export TRANSACTION_AWAITING_COUNT="$PWD/awaiting-count"
export TRANSACTION_HANG_PID="$PWD/hung-ssh.pid"
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
  rm -f "$TRANSACTION_AWAITING_COUNT"
  : >"$TRANSACTION_STATUS_COUNT"
  if [[ "${TRANSACTION_LOST:-}" == 1 ]]; then
    exit 255
  fi
  printf '{}\n'
elif [[ "$remote" == *" confirm "* ]]; then
  [[ "$remote" == *"'"$(cat "$TRANSACTION_ID")"'"* ]]
  # The executor persists `phase: succeeded` before it prints the reply, so a
  # slow reply still carries the result.
  sleep "${TRANSACTION_CONFIRM_DELAY_SECONDS:-0}"
  touch "$TRANSACTION_ID.confirmed"
  if [[ "${TRANSACTION_LOST_CONFIRM:-}" == 1 ]]; then exit 255; fi
  printf '{"version":1,"id":"%s","phase":"succeeded"}\n' "$(cat "$TRANSACTION_ID")"
elif [[ "$remote" == *status* ]]; then
  observations=0
  if [[ -s "$TRANSACTION_STATUS_COUNT" ]]; then
    observations=$(cat "$TRANSACTION_STATUS_COUNT")
  fi
  observations=$((observations + 1))
  printf '%s' "$observations" >"$TRANSACTION_STATUS_COUNT"
  sleep "${TRANSACTION_STATUS_DELAY_SECONDS:-0}"
  # The target can also become unobservable once the confirmation landed.
  if [[ "${TRANSACTION_STATUS_LOST_AFTER_CONFIRM:-}" == 1 && -e "$TRANSACTION_ID.confirmed" ]]; then
    echo "status transport failure after confirmation" >&2
    exit 255
  fi
  # A busy target drops status round trips and, while it cannot observe the
  # activation unit, reports the transaction rather than its progress.
  if ((observations <= ${TRANSACTION_STATUS_DROPS:-0})); then
    echo "transient status transport failure" >&2
    exit 255
  fi
  if ((observations <= ${TRANSACTION_UNKNOWN_STATUSES:-0})); then
    printf '{"version":1,"id":"%s","outcome":"unknown","observationError":"cannot observe activation service"}\n' \
      "$(cat "$TRANSACTION_ID")"
    exit 0
  fi
  # A status round trip that hangs on the target keeps the local ssh blocked.
  if ((observations <= ${TRANSACTION_STATUS_HANGS:-0})); then
    printf '%s' "$$" >"$TRANSACTION_HANG_PID"
    # exec keeps the recorded PID on the process that actually sleeps, so the
    # controller abandoning the attempt kills the sleeper and nothing else.
    exec sleep "${TRANSACTION_STATUS_HANG_SECONDS:-31}"
  fi
  # A real executor fault, distinct from a transport failure: it exits with a
  # code OpenSSH never uses for a dropped connection.
  if [[ -n "${TRANSACTION_STATUS_FAILURE_CODE:-}" ]]; then
    echo "executor cannot read the transaction" >&2
    exit "$TRANSACTION_STATUS_FAILURE_CODE"
  fi
  if [[ "${TRANSACTION_CONFIRM:-}" == 1 && ! -e "$TRANSACTION_ID.confirmed" ]]; then
    awaiting=$(cat "$TRANSACTION_AWAITING_COUNT" 2>/dev/null || echo 0)
    awaiting=$((awaiting + 1))
    printf '%s' "$awaiting" >"$TRANSACTION_AWAITING_COUNT"
    # An unconfirmed transaction is rolled back by the target-side watchdog.
    if ((awaiting > ${TRANSACTION_UNCONFIRMED_ATTEMPTS:-1000000})); then
      printf '{"version":1,"id":"%s","outcome":"rolled-back","error":"confirmation never arrived"}\n' \
        "$(cat "$TRANSACTION_ID")"
      exit 0
    fi
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

# `unknown` is no longer a verdict; the bounded persistent case below covers it.
for outcome in failed rolled-back rollback-failed; do
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

# A lost confirmation reply is reconciled with the target's own record: the
# executor persists `phase: succeeded` before replying, so a status read is the
# verdict.
if ! TRANSACTION_CONFIRM=1 TRANSACTION_LOST_CONFIRM=1 \
  devenv machines deploy server --yes >lost-confirm.log 2>&1; then
  cat lost-confirm.log
  echo "a confirmation whose reply was lost aborted a successful deployment"
  exit 1
fi
grep -q ' confirm ' "$TRANSACTION_LOG"

# An unanswered confirmation is not itself a success: without a matching
# target record the transaction's own outcome decides.
if TRANSACTION_CONFIRM=1 TRANSACTION_LOST_CONFIRM=1 TRANSACTION_OUTCOME=rolled-back \
  devenv machines deploy server --yes >lost-confirm-outcome.log 2>&1; then
  echo "a lost confirmation reply reported success without a target record"
  exit 1
fi
grep -q 'rolled back' lost-confirm-outcome.log

# A gap in observation is not a decision: the target cannot see the activation
# unit while it starts, switches, checks, or reboots, and a status round trip
# can also fail while the target is busy. The transaction keeps running on the
# target and the controller must keep watching it.
transient_gap() {
  local setting=$1 log=$2 message=$3
  : >"$TRANSACTION_LOG"
  if ! env TRANSACTION_CONFIRM=1 "$setting" \
    devenv machines deploy server --yes >"$log" 2>&1; then
    cat "$log"
    echo "$message"
    exit 1
  fi
  grep -q ' confirm ' "$TRANSACTION_LOG"
}
transient_gap TRANSACTION_UNKNOWN_STATUSES=3 transient-unknown.log \
  "a transient unknown observation aborted the transaction"
transient_gap TRANSACTION_STATUS_DROPS=2 transient-status.log \
  "a brief status transport failure aborted the transaction"

# A gap that never closes still fails closed, and nothing is confirmed. The
# window is long enough for several full SSH attempts, but it is bounded.
: >"$TRANSACTION_LOG"
SECONDS=0
if TRANSACTION_CONFIRM=1 TRANSACTION_UNKNOWN_STATUSES=100000 \
  devenv machines deploy server --yes >persistent-unknown.log 2>&1; then
  echo "a transaction without a closing observation reported success"
  exit 1
fi
grep -q 'Lost observation' persistent-unknown.log
if ((SECONDS < 85)); then
  echo "the observation window closed before a full retry budget elapsed"
  exit 1
fi
if grep -q ' confirm ' "$TRANSACTION_LOG"; then
  echo "an unobserved transaction was confirmed"
  exit 1
fi

# One status round trip that hangs must not spend the whole observation gap:
# the controller allows it the standard SSH request timeout, abandons that
# attempt, keeps watching the same transaction, and acts on the next answer.
# A hung local ssh is dropped with the attempt.
: >"$TRANSACTION_LOG"
rm -f "$TRANSACTION_HANG_PID"
SECONDS=0
if ! TRANSACTION_CONFIRM=1 TRANSACTION_STATUS_HANGS=1 \
  devenv machines deploy server --yes >hung-status.log 2>&1; then
  cat hung-status.log
  echo "a hung status round trip aborted a running transaction"
  exit 1
fi
if ((SECONDS < 28)); then
  echo "the controller cut a status attempt short of the SSH request timeout"
  exit 1
fi
if ((SECONDS >= 45)); then
  echo "the controller waited out the hung status instead of observing again"
  exit 1
fi
grep -q ' confirm ' "$TRANSACTION_LOG"
hung_pid=$(cat "$TRANSACTION_HANG_PID")
for _ in 1 2 3 4 5 6 7 8 9 10; do
  kill -0 "$hung_pid" 2>/dev/null || break
  sleep 0.2
done
if kill -0 "$hung_pid" 2>/dev/null; then
  kill "$hung_pid" 2>/dev/null || true
  echo "a timed-out status round trip left its local ssh running"
  exit 1
fi

# A status reply slower than a few seconds is still an answer: the standard
# SSH request timeout bounds each attempt.
: >"$TRANSACTION_LOG"
SECONDS=0
if ! TRANSACTION_CONFIRM=1 TRANSACTION_STATUS_DELAY_SECONDS=6 \
  devenv machines deploy server --yes >slow-status.log 2>&1; then
  cat slow-status.log
  echo "a six-second status reply aborted the transaction"
  exit 1
fi
if ((SECONDS >= 20)); then
  echo "the controller did not confirm after a six-second status reply"
  exit 1
fi
grep -q ' confirm ' "$TRANSACTION_LOG"

# The executor persists the terminal state before answering `confirm`, so a
# slow confirmation is the result even when every later status poll fails.
: >"$TRANSACTION_LOG"
if ! TRANSACTION_CONFIRM=1 TRANSACTION_CONFIRM_DELAY_SECONDS=29 \
  TRANSACTION_STATUS_LOST_AFTER_CONFIRM=1 \
  devenv machines deploy server --yes >slow-confirm.log 2>&1; then
  cat slow-confirm.log
  echo "a slow successful confirmation was reported as lost observation"
  exit 1
fi
if grep -q 'Lost observation' slow-confirm.log; then
  echo "a successful confirmation was reported as lost observation"
  exit 1
fi
if [[ "$(grep -c ' confirm ' "$TRANSACTION_LOG")" != 1 ]]; then
  echo "an authoritative confirmation was requested more than once"
  exit 1
fi

# A confirmation that outlives the SSH bound must not be sent a second time:
# another confirm can collide with the handler that still holds the executor's
# lock. The controller reads the same transaction back and lets the armed
# watchdog decide.
: >"$TRANSACTION_LOG"
if TRANSACTION_CONFIRM=1 TRANSACTION_CONFIRM_DELAY_SECONDS=40 \
  TRANSACTION_UNCONFIRMED_ATTEMPTS=3 \
  devenv machines deploy server --yes >confirm-timeout.log 2>&1; then
  echo "an unconfirmed transaction reported success"
  exit 1
fi
if [[ "$(grep -c ' confirm ' "$TRANSACTION_LOG")" != 1 ]]; then
  echo "an unanswered confirmation was requested more than once"
  exit 1
fi
grep -q 'confirmation never arrived' confirm-timeout.log

# A nonzero executor exit is a fault, not a gap: it is reported at once
# instead of being retried until the observation gap closes.
: >"$TRANSACTION_LOG"
if TRANSACTION_CONFIRM=1 TRANSACTION_STATUS_FAILURE_CODE=97 \
  devenv machines deploy server --yes >status-failure.log 2>&1; then
  echo "an executor failure reported success"
  exit 1
fi
grep -q 'could not report transaction' status-failure.log
if [[ "$(cat "$TRANSACTION_STATUS_COUNT")" != 1 ]]; then
  echo "an executor failure was retried as if it were a gap"
  exit 1
fi
if grep -q ' confirm ' "$TRANSACTION_LOG"; then
  echo "a failed executor read was confirmed"
  exit 1
fi

# The unreleased direct NixOS bypass has been removed.
: > "$TRANSACTION_LOG"
if devenv machines deploy server --legacy > removed-flag.log 2>&1; then exit 1; fi
grep -q "unexpected argument '--legacy'" removed-flag.log
test ! -s "$TRANSACTION_LOG"
