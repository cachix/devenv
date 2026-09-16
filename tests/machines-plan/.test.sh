#!/usr/bin/env bash
set -euo pipefail

mkdir -p mock-bin
test_python=$(devenv build outputs.test-python | jq -er '."outputs.test-python"')/bin/python3
export PLAN_LOG="$PWD/ssh.log"
export PLAN_NEW
PLAN_NEW=$(devenv build machines.server.build.nixos | jq -er '."machines.server.build.nixos"')
export PLAN_OLD=/nix/store/00000000000000000000000000000000-old-system
export PLAN_REMOVED=/nix/store/11111111111111111111111111111111-old-dependency
export PLAN_SHARED
PLAN_SHARED=$(cat "$PLAN_NEW/reference")
export REAL_NIX_STORE
REAL_NIX_STORE=$(command -v nix-store)

cat >mock-bin/ssh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" >> "$PLAN_LOG"
remote="${!#}"
if [[ "$remote" == *devenv-machine-facts-v1* ]]; then
  if [[ "${PLAN_UNCHANGED:-}" == 1 ]]; then printf '%s\n' "$PLAN_NEW"; else printf '%s\n' "$PLAN_OLD"; fi
  printf '%s\n' "${PLAN_FACTS:-null}"
  exit 0
fi
if [[ "${APPLY_TEST:-}" == 1 && "$remote" != *'devenv-machine-plan-v1'* ]]; then
  if [[ "$remote" == *' start '* ]]; then
    [[ "$remote" == *"'$PLAN_NEW'"* ]]
    [[ "$remote" == *"--expected-system '$PLAN_OLD'"* ]]
    [[ "$remote" == *"--expected-profile '$PLAN_NEW'"* ]]
    [[ "$remote" == *"'$PLAN_EXECUTOR/bin/devenv-machine-deploy'"* ]]
    request=" start '([A-Za-z0-9-]+)'"
    [[ "$remote" =~ $request ]]
    printf '%s' "${BASH_REMATCH[1]}" > "$PLAN_LOG.id"
    if [[ "${APPLY_REJECT:-}" == 1 ]]; then exit 94; fi
    printf '{}\n'
  elif [[ "$remote" == *' status' ]]; then
    printf '{"version":1,"id":"%s","outcome":"succeeded"}\n' "$(cat "$PLAN_LOG.id")"
  else
    exit 95
  fi
  exit 0
fi
# Refuse any mutation, including copying or installing an executor.
[[ "$remote" == *'devenv-machine-plan-v1'* ]]
[[ "$remote" == *'nix-store --query --requisites'* ]]
[[ "$remote" != *'nix-env'* && "$remote" != *'systemd-run'* && "$remote" != *'switch-to-configuration'* ]]
if [[ "${PLAN_FAIL:-}" == 1 ]]; then exit 97; fi
if [[ "${PLAN_MALFORMED:-}" == 1 ]]; then printf 'unexpected login banner\n'; exit 0; fi
if [[ ( "${APPLY_DRIFT_AFTER_COPY:-}" == 1 && -e "$PLAN_LOG.copied" ) || "${PLAN_UNCHANGED:-}" == 1 || ( "${APPLY_SECOND_STALE:-}" == 1 && "$*" == *second.invalid* ) ]]; then
  printf 'devenv-machine-plan-v1\n%s\n%s\n' "$PLAN_NEW" "$PLAN_NEW"
  exec "$REAL_NIX_STORE" --query --requisites "$PLAN_NEW"
fi
printf 'devenv-machine-plan-v1\n%s\n%s\n%s\n%s\n%s\n' \
  "$PLAN_OLD" "$PLAN_NEW" "$PLAN_OLD" "$PLAN_REMOVED" "$PLAN_SHARED"
EOF
cat >mock-bin/nix <<'EOF'
#!/usr/bin/env bash
if [[ "${APPLY_TEST:-}" == 1 && "$1" == copy ]]; then
  printf 'copy %s\n' "${!#}" >> "$PLAN_LOG"
  touch "$PLAN_LOG.copied"
  if [[ "${APPLY_COPY_FAIL_SECOND:-}" == 1 && "$*" == *second.invalid* ]]; then exit 93; fi
  exit 0
fi
echo "unexpected external nix operation: $*" >&2
exit 98
EOF
cat >mock-bin/nix-store <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == --query && "$2" == --requisites ]]
if [[ "${PLAN_LOCAL_FAIL:-}" == 1 ]]; then exit 96; fi
if [[ "${PLAN_LOCAL_INCOMPLETE:-}" == 1 ]]; then exit 0; fi
exec "$REAL_NIX_STORE" "$@"
EOF
chmod +x mock-bin/*
export PATH="$PWD/mock-bin:$PATH"

devenv machines plan --json server > plan.json
jq -e --arg old "$PLAN_OLD" --arg new "$PLAN_NEW" --arg removed "$PLAN_REMOVED" '
  .version == 3 and .scope == "nixos" and
  (.machines.server | .target == "reader@preview.invalid" and
    .currentSystem == $old and .currentProfile == $new and .requestedSystem == $new and
    .systemChanged and (.profileChanged | not) and
    .addedStorePaths == [$new] and .removedStorePaths == ([$old, $removed] | sort) and
    .unplannedRoles == [])' plan.json
grep -qx 2222 "$PLAN_LOG"

PLAN_UNCHANGED=1 devenv machines plan --json > unchanged.json
jq -e '.machines | keys == ["server"]' unchanged.json
jq -e '.machines.server | (.systemChanged | not) and (.profileChanged | not) and .addedStorePaths == [] and .removedStorePaths == []' unchanged.json

export PLAN_EXECUTOR
PLAN_EXECUTOR=$(jq -er '.machines.server.executor' plan.json)
devenv machines plan server > saved-plan.log
saved_plan=$(sed -n 's/^Saved plan: //p' saved-plan.log)
test -n "$saved_plan"
test -L ".devenv/machine-plans/$saved_plan/server-system"
test -L ".devenv/machine-plans/$saved_plan/server-executor"

: > "$PLAN_LOG"
if APPLY_TEST=1 devenv machines deploy server </dev/null > noninteractive.log 2>&1; then exit 1; fi
grep -q 'needs confirmation' noninteractive.log
if grep -q '^copy ' "$PLAN_LOG"; then exit 1; fi
APPLY_TEST=1 "$test_python" .test-pty.py
: > "$PLAN_LOG"
APPLY_TEST=1 devenv machines deploy server --yes
grep -qx "copy $PLAN_NEW" "$PLAN_LOG"

# Selection is validated in full before contacting any target.
for args in 'server missing' 'server server' 'local'; do
  : > "$PLAN_LOG"
  # Deliberate splitting supplies separate CLI names.
  if devenv machines plan --json $args > rejected.json 2> rejected.log; then exit 1; fi
  test ! -s "$PLAN_LOG"
  test ! -s rejected.json
done

if PLAN_FAIL=1 devenv machines plan --json server > failed.json 2> failed.log; then exit 1; fi
test ! -s failed.json
grep -q 'Could not observe' failed.log
if PLAN_MALFORMED=1 devenv machines plan --json server > malformed.json 2> malformed.log; then exit 1; fi
test ! -s malformed.json
grep -q 'Invalid target response' malformed.log

for failure in PLAN_LOCAL_FAIL PLAN_LOCAL_INCOMPLETE; do
  : > "$PLAN_LOG"
  if env "$failure=1" devenv machines plan --json server > local-failed.json 2> local-failed.log; then exit 1; fi
  test ! -s local-failed.json
  test ! -s "$PLAN_LOG"
done

# Applying uses only the reviewed paths, even when current roles cannot build.
export PLAN_EXECUTOR
PLAN_EXECUTOR=$(jq -er '.machines.server.executor' plan.json)
cat > devenv.local.nix <<'EOF'
{ lib, ... }: {
  machines.server.build.nixos = lib.mkForce (throw "apply rebuilt the system");
  machines.server.build.deployer = lib.mkForce (throw "apply rebuilt the executor");
  machines.zserver = {
    target.host = "reader@second.invalid";
    target.sshOpts = [ "-p" "2222" ];
    hardware.facter = null;
    nixos = {};
  };
}
EOF
: > "$PLAN_LOG"
APPLY_TEST=1 devenv machines apply "$saved_plan"
grep -qx "copy $PLAN_NEW" "$PLAN_LOG"
grep -qx "copy $PLAN_EXECUTOR" "$PLAN_LOG"

# Stale preflight rejects before copying. A target-side rejection after copy
# cannot be mistaken for success or retried as an unguarded activation.
: > "$PLAN_LOG"
if APPLY_TEST=1 PLAN_UNCHANGED=1 devenv machines apply plan.json > stale.log 2>&1; then exit 1; fi
grep -q 'Stale deployment plan' stale.log
if grep -q '^copy ' "$PLAN_LOG"; then exit 1; fi
if APPLY_TEST=1 APPLY_REJECT=1 devenv machines apply plan.json > rejected-apply.log 2>&1; then exit 1; fi
grep -q 'may have started' rejected-apply.log

for edit in '.version = 1' '.machines.server.target = "other.invalid"' \
  '.machines.server.requestedSystem += "/bin"' '.machines = {}' \
  '.machines.server.executor = "/nix/store/00000000000000000000000000000000-missing"'; do
  jq "$edit" plan.json > invalid-plan.json
  : > "$PLAN_LOG"
  if APPLY_TEST=1 devenv machines apply invalid-plan.json > invalid-apply.log 2>&1; then exit 1; fi
  test ! -s "$PLAN_LOG"
done

jq '.machines.zserver = .machines.server | .machines.zserver.target = "reader@second.invalid"' plan.json > fleet.json
: > "$PLAN_LOG"
if APPLY_TEST=1 APPLY_SECOND_STALE=1 devenv machines apply fleet.json > fleet-stale.log 2>&1; then exit 1; fi
grep -q 'Stale deployment plan for zserver' fleet-stale.log
if grep -q '^copy ' "$PLAN_LOG"; then exit 1; fi

: > "$PLAN_LOG"
if APPLY_TEST=1 APPLY_REJECT=1 devenv machines apply fleet.json > fleet-failed.log 2>&1; then exit 1; fi
test "$(grep -c '^copy ' "$PLAN_LOG")" -eq 4
test "$(grep -c ' start ' "$PLAN_LOG")" -eq 1

for fault in APPLY_COPY_FAIL_SECOND APPLY_DRIFT_AFTER_COPY; do
  : > "$PLAN_LOG"
  rm -f "$PLAN_LOG.copied"
  if env APPLY_TEST=1 "$fault=1" devenv machines apply fleet.json > "$fault.log" 2>&1; then exit 1; fi
  if grep -q ' start ' "$PLAN_LOG"; then exit 1; fi
done

# A disabled management path blocks both --yes and saved-plan application.
cat > devenv.local.nix <<'NIX'
{ lib, ... }: {
  machines.server.nixos.services.openssh.enable = lib.mkForce false;
  machines.server.nixos.services.openssh.settings.PermitRootLogin = lib.mkForce "no";
}
NIX
devenv machines plan --json server > blocked-plan.json
jq -e '.machines.server.findings | any(.code == "ssh-disabled" and .severity == "error") and any(.code == "root-login-disabled" and .severity == "error")' blocked-plan.json
: > "$PLAN_LOG"
if APPLY_TEST=1 devenv machines deploy server --yes > blocked-deploy.log 2>&1; then exit 1; fi
grep -q 'Access checks block deployment' blocked-deploy.log
if grep -q '^copy ' "$PLAN_LOG"; then exit 1; fi
: > "$PLAN_LOG"
if APPLY_TEST=1 devenv machines apply blocked-plan.json > blocked-apply.log 2>&1; then exit 1; fi
grep -q 'Access checks block deployment' blocked-apply.log
test ! -s "$PLAN_LOG"
# Forging findings cannot bypass recomputation from the saved facts.
jq '.machines.server.findings = []' blocked-plan.json > forged-plan.json
if APPLY_TEST=1 devenv machines apply forged-plan.json > forged.log 2>&1; then exit 1; fi
grep -q 'Inconsistent access findings' forged.log
