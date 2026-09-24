#!/usr/bin/env bash
set -euo pipefail
export MIXED_LOG="$PWD/operations.log"
export MIXED_OLD=/nix/store/00000000000000000000000000000000-old
export REAL_NIX
REAL_NIX=$(command -v nix)
mkdir mock-bin
cat > mock-bin/nix <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" != copy ]]; then exec "$REAL_NIX" "$@"; fi
printf 'copy %s %s\n' "$3" "$4" >> "$MIXED_LOG"
if [[ "${FAIL_COPY:-}" == 1 && "$3" == *home.invalid* ]]; then exit 77; fi
MOCK
cat > mock-bin/ssh <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
remote="${!#}"
host=unknown
for arg in "$@"; do if [[ "$arg" == *@*.invalid ]]; then host="$arg"; fi; done
if [[ "$remote" == *devenv-machine-plan-v1* ]]; then
  printf 'observe %s\n' "$host" >> "$MIXED_LOG"
  printf 'devenv-machine-plan-v1\n%s\n%s\n%s\n' "$MIXED_OLD" "$MIXED_OLD" "$MIXED_OLD"
elif [[ "$remote" == *devenv-machine-facts-v1* ]]; then
  printf '%s\nnull\n' "$MIXED_OLD"
elif [[ "$remote" == *devenv-machine-preflight* ]]; then
  printf 'preflight %s\n' "$host" >> "$MIXED_LOG"
  if [[ "${FAIL_PREFLIGHT:-}" == 1 && "$host" == *home.invalid ]]; then exit 77; fi
elif [[ "$remote" == *" start "* ]]; then
  printf 'start %s\n' "$host" >> "$MIXED_LOG"
  request="start '([A-Za-z0-9-]+)'"
  [[ "$remote" =~ $request ]]
  printf '%s' "${BASH_REMATCH[1]}" > "$MIXED_LOG.id"
  if [[ "${FAIL_SYSTEM:-}" == 1 ]]; then exit 77; fi
  printf '{}\n'
elif [[ "$remote" == *" status" ]]; then
  printf '{"version":1,"id":"%s","outcome":"succeeded"}\n' "$(cat "$MIXED_LOG.id")"
elif [[ "$remote" == *nix-env* ]]; then
  printf 'darwin %s\n' "$host" >> "$MIXED_LOG"
elif [[ "$remote" == *"/activate'" ]]; then
  printf 'home %s\n' "$host" >> "$MIXED_LOG"
  if [[ "${FAIL_HOME:-}" == 1 && "$host" == *linux.invalid ]]; then exit 77; fi
else
  echo "unexpected SSH command: $remote" >&2
  exit 98
fi
MOCK
chmod +x mock-bin/*
export PATH="$PWD/mock-bin:$PATH"
: > "$MIXED_LOG"
# A single review includes all remote roles, but excludes local-only entries.
devenv machines plan --json > plan.json
jq -e '.version == 4 and .scope == "fleet" and (.machines | keys == ["a-linux", "b-mac", "c-home"]) and
  (.machines["a-linux"] | .nixos != null and .homeManager != null) and
  (.machines["b-mac"] | .nixDarwin != null and .homeManager != null) and
  (.machines["c-home"] | .nixos == null and .homeManager != null)' plan.json
if grep -Eq '^(copy|start|darwin|home) ' "$MIXED_LOG"; then exit 1; fi

# Bare deploy prepares the whole mixed fleet, then applies every declared role.
: > "$MIXED_LOG"
devenv machines deploy --yes
awk '/^copy / {copies++} /^(start|darwin|home) / {if (copies != 6) exit 1; activated++} END {if (activated != 5) exit 1}' "$MIXED_LOG"
grep -E '^(start|darwin|home) ' "$MIXED_LOG" > actual.log
cat > expected.log <<'EOF'
start root@linux.invalid
home root@linux.invalid
darwin admin@mac.invalid
home admin@mac.invalid
home user@home.invalid
EOF
diff -u expected.log actual.log

# A late role build failure cannot cause an earlier role to activate or copy.
cat > devenv.local.nix <<'NIX'
{ lib, ... }: { machines.c-home.build.home-manager = lib.mkForce (throw "late role build failed"); }
NIX
: > "$MIXED_LOG"
if devenv machines deploy --yes > build-failed.log 2>&1; then exit 1; fi
grep -q 'late role build failed' build-failed.log
if grep -Eq '^(copy|start|darwin|home) ' "$MIXED_LOG"; then exit 1; fi
printf '{}\n' > devenv.local.nix

# Every pinned role is rooted and application never reevaluates its build.
devenv machines plan > summary.log
grep -q 'transactional rollback' summary.log
grep -q 'direct activation, no automatic rollback' summary.log
saved=$(sed -n 's/^Saved plan: //p' summary.log)
for root in a-linux-system a-linux-executor a-linux-home-manager b-mac-nix-darwin b-mac-home-manager c-home-home-manager; do
  test -L ".devenv/machine-plans/$saved/$root"
done
cat > devenv.local.nix <<'NIX'
{ lib, ... }: {
  machines.a-linux.build.nixos = lib.mkForce (throw "rebuilt linux");
  machines.a-linux.build.deployer = lib.mkForce (throw "rebuilt executor");
  machines.a-linux.build.home-manager = lib.mkForce (throw "rebuilt linux home");
  machines.b-mac.build.nix-darwin = lib.mkForce (throw "rebuilt darwin");
  machines.b-mac.build.home-manager = lib.mkForce (throw "rebuilt mac home");
  machines.c-home.build.home-manager = lib.mkForce (throw "rebuilt standalone home");
}
NIX
: > "$MIXED_LOG"
devenv machines apply "$saved" --max-concurrent 2
test "$(grep -Ec '^(start|darwin|home) ' "$MIXED_LOG")" -eq 5

for fault in FAIL_COPY FAIL_PREFLIGHT; do
  : > "$MIXED_LOG"
  if env "$fault=1" devenv machines apply plan.json > "$fault.log" 2>&1; then exit 1; fi
  if grep -Eq '^(start|darwin|home) ' "$MIXED_LOG"; then exit 1; fi
done
# A local role participates in the same preparation barrier.
printf '{}\n' > devenv.local.nix
devenv machines plan --json a-linux b-mac c-home d-local > including-local.json
: > "$MIXED_LOG"
if FAIL_COPY=1 devenv machines apply including-local.json > local-barrier.log 2>&1; then exit 1; fi
if grep -Eq '^(start|darwin|home|local-activation)' "$MIXED_LOG"; then exit 1; fi

# Validate pinned direct output paths and concurrency before remote contact.
for change in '.machines["c-home"].homeManager += "/child"' '.machines["a-linux"].nixos = null'; do
  jq "$change" plan.json > invalid.json
  : > "$MIXED_LOG"
  if devenv machines apply invalid.json > invalid.log 2>&1; then exit 1; fi
  test ! -s "$MIXED_LOG"
done
: > "$MIXED_LOG"
if devenv machines apply plan.json --max-concurrent 0 > zero.log 2>&1; then exit 1; fi
test ! -s "$MIXED_LOG"

# A failed NixOS submission cannot run its home role or start later machines.
: > "$MIXED_LOG"
if FAIL_SYSTEM=1 devenv machines apply plan.json > system-failed.log 2>&1; then exit 1; fi
if grep -Eq '^(darwin|home) ' "$MIXED_LOG"; then exit 1; fi
# Home failure preserves the confirmed system and reports the partial result.
: > "$MIXED_LOG"
if FAIL_HOME=1 devenv machines apply plan.json > home-failed.log 2>&1; then exit 1; fi
grep -q 'completed system activation remains applied' home-failed.log
if grep -q '^darwin ' "$MIXED_LOG"; then exit 1; fi
# Concurrent failures drain the active batch but never start the next batch.
: > "$MIXED_LOG"
if FAIL_HOME=1 devenv machines apply plan.json --max-concurrent 2 > batch-failed.log 2>&1; then exit 1; fi
grep -q '^home admin@mac.invalid$' "$MIXED_LOG"
if grep -q '^home user@home.invalid$' "$MIXED_LOG"; then exit 1; fi
# Role changes invalidate a saved plan before any remote contact.
cat > devenv.local.nix <<'NIX'
{ lib, ... }: { machines.b-mac.home-manager = lib.mkForce null; }
NIX
: > "$MIXED_LOG"
if devenv machines apply plan.json > changed.log 2>&1; then exit 1; fi
grep -q 'no longer matches the planned roles' changed.log
test ! -s "$MIXED_LOG"
# Local home-manager remains explicitly selectable and uses the same review.
printf '{}\n' > devenv.local.nix
: > "$MIXED_LOG"
devenv machines deploy d-local --yes
grep -qx local-activation "$MIXED_LOG"
# Explicit subsets do not activate other machines; noninteractive runs need --yes.
: > "$MIXED_LOG"
if devenv machines deploy c-home </dev/null > confirmation.log 2>&1; then exit 1; fi
grep -q 'Deployment needs confirmation' confirmation.log
test ! -s "$MIXED_LOG"
devenv machines deploy c-home --yes
test "$(grep -Ec '^(start|darwin|home) ' "$MIXED_LOG")" -eq 1
