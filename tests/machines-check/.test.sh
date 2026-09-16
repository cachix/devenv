#!/usr/bin/env bash
set -euo pipefail
mkdir -p mock-bin
export CHECK_LOG="$PWD/commands.log"
cat > mock-bin/ssh <<'SSH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$CHECK_LOG"
[[ "${!#}" == *devenv-machine-facts-v1* ]]
printf '/nix/store/00000000000000000000000000000000-current\n'
if [[ -f current.json ]]; then cat current.json; else printf 'null\n'; fi
SSH
for tool in nix nix-store; do
  cat > "mock-bin/$tool" <<'MOCK'
#!/usr/bin/env bash
echo "unexpected build/store command" >&2
exit 98
MOCK
done
chmod +x mock-bin/*
export PATH="$PWD/mock-bin:$PATH"
devenv machines check server --json > check.json
jq -e '.scope == "configuration-access" and (.buildsPerformed | not) and (.closureCompared | not) and (.activationHealthChecked | not)' check.json
jq -e '.machines.server | .findings | any(.code == "current-facts-unavailable")' check.json
jq '.machines.server.requestedFacts' check.json > current.json
key_hash="sha256:$(printf '%s' 'ssh-ed25519 AAAATEST public-fixture' | sha256sum | cut -d' ' -f1)"
jq -e --arg key "$key_hash" '.version == 1 and .ssh.ports == [2222] and .ssh.rootLogin == "prohibit-password" and .ssh.adminKeys.root == [$key] and .recoveryEnabled' current.json
if grep -q 'AAAATEST' current.json; then exit 1; fi
# Build only the facts document, then verify it is identical to evaluation.
facts=$(devenv build outputs.facts | jq -er '."outputs.facts"')
jq -S . "$facts" > installed.json
jq -S . current.json > evaluated.json
diff -u evaluated.json installed.json

devenv machines check server > summary.log
grep -q 'No builds, closure comparison, or activation health checks' summary.log
test ! -d .devenv/machine-plans
cat > devenv.local.nix <<'NIX'
{ lib, ... }: {
  machines.server.nixos = {
    services.openssh.enable = lib.mkForce false;
    services.openssh.settings.PermitRootLogin = lib.mkForce "no";
    services.openssh.authorizedKeysInHomedir = lib.mkForce true;
    networking.firewall.extraCommands = "echo custom-rule";
    systemd.services.devenv-machines-recover.enable = lib.mkForce false;
  };
}
NIX
if devenv machines check server --json > blocked.json; then exit 1; fi
jq -e '.machines.server.findings | any(.code == "ssh-disabled" and .severity == "error") and any(.code == "root-login-disabled" and .severity == "error") and any(.code == "access-analysis-incomplete") and any(.code == "recovery-disabled")' blocked.json
test ! -d .devenv/machine-plans
: > "$CHECK_LOG"
if devenv machines check server missing > invalid.json 2>invalid.log; then exit 1; fi
test ! -s "$CHECK_LOG"

# Evaluation itself must not sneak in a build through import-from-derivation.
cat > devenv.local.nix <<'NIX'
{ config, ... }: {
  machines.server.nixos = {
    imports = [ ({ pkgs, lib, ... }: {
      networking.hostName = lib.mkForce (builtins.readFile (
        pkgs.runCommand "forbidden-access-check-ifd" { nonce = config.devenv.root; } "printf forbidden > $out"
      ));
    }) ];
  };
}
NIX
if devenv --nix-option max-jobs 1 machines check server > ifd.json 2>ifd.log; then exit 1; fi
grep -Eq 'Unable to start any build|max-jobs|cannot build' ifd.log || { cat ifd.log; exit 1; }
