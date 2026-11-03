#!/usr/bin/env bash
set -euo pipefail

system=$(nix eval --raw --impure --expr builtins.currentSystem)
darwin_system=aarch64-darwin
if [[ "$system" == *-darwin ]]; then darwin_system=$system; fi
devenv eval machinesMeta.server.system | jq -e --arg system "${system/darwin/linux}" '.["machinesMeta.server.system"] == $system'
devenv eval machinesMeta.mac.system | jq -e --arg system "$darwin_system" '.["machinesMeta.mac.system"] == $system'
devenv eval machinesMeta.home.hasHomeManager | jq -e '.["machinesMeta.home.hasHomeManager"] == true'

home=$(devenv build machines.home.build.home-manager | jq -er '."machines.home.build.home-manager"')
grep -q "user=jdoe" "$(readlink -f "$home/activate")"
grep -qx "from the home configuration" "$home/home-files/configuration-marker"
[[ -e "$home/home-path" ]]

devenv eval machines.mac.build.nix-darwin.drvPath | jq -e '.["machines.mac.build.nix-darwin.drvPath"] | endswith(".drv")'
marker=$(devenv build outputs.darwinMarker | jq -er '."outputs.darwinMarker"')
grep -qx "from the Darwin configuration" "$marker"

cat >devenv.local.nix <<'LOCAL'
{ ... }: {
  machines.home.system = "riscv64-linux";
}
LOCAL
if devenv eval machinesMeta.home.system 2>architecture.err; then exit 1; fi
grep -q "machines.home.system must match every supplied configuration's architecture" architecture.err
rm devenv.local.nix

if [[ "$system" == *-darwin ]]; then
  mac=$(devenv build machines.mac.build.nix-darwin | jq -er '."machines.mac.build.nix-darwin"')
  [[ -x "$mac/activate" ]]
  exit 0
fi

toplevel=$(devenv build machines.server.build.nixos | jq -er '."machines.server.build.nixos"')
if [[ "$(cat "$toplevel/etc/configuration-marker")" != "from the configuration" ]]; then
  echo "devenv rebuilt the configuration instead of extending it"
  exit 1
fi
if [[ ! -e "$toplevel/etc/devenv/machine-facts.json" ]]; then
  echo "devenv did not add its modules to the configuration"
  exit 1
fi

if devenv build machines.server.build.diskoScript 2>disko.err; then
  echo "expected a configuration without disko to have no disko script"
  exit 1
fi
grep -q "needs disko in the NixOS configuration" disko.err

mkdir -p mock-bin
cat >mock-bin/ssh <<'MOCK'
#!/usr/bin/env bash
exit 97
MOCK
chmod +x mock-bin/ssh
if PATH="$PWD/mock-bin:$PATH" devenv machines install --phases disko server 2>install.err; then
  echo "expected install without disko to be refused"
  exit 1
fi
grep -q "does not include disko" install.err

rebuilt=$(devenv build outputs.standalone | jq -er '."outputs.standalone"')
jq -e '.secrets == {"TOKEN": "TOKEN"}' "$rebuilt/etc/devenv/secrets.json" >/dev/null
deployed=$(devenv build machines.standalone.build.nixos | jq -er '."machines.standalone.build.nixos"')
jq -e '.secrets == {"TOKEN": "TOKEN"}' "$deployed/etc/devenv/secrets.json" >/dev/null
devenv eval machines.standalone.deploy.secrets | jq -e '.["machines.standalone.deploy.secrets"] == {"TOKEN": "TOKEN"}'

compatible=$(devenv build machines.compatible.build.nixos | jq -er '."machines.compatible.build.nixos"')
grep -qx "from the pinned modules" "$compatible/etc/module-version-marker"
jq -e '.secrets == {"TOKEN": "TOKEN"}' "$compatible/etc/devenv/secrets.json" >/dev/null

if devenv build machines.skewed.build.nixos 2>skewed.err; then
  echo "expected modules from another devenv version to be rejected"
  exit 1
fi
grep -q "incompatible devenv machine module interface" skewed.err

cat >devenv.local.nix <<'LOCAL'
{ config, ... }: {
  machines.custom.nixos = config.machines.standalone.nixos.extendModules {
    modules = [{ devenv.secrets.TOKEN.path = "/var/lib/app/token"; }];
  };
}
LOCAL
if devenv eval machines.custom._nixosEval.config.devenv.secrets.TOKEN.path 2>custom.err; then exit 1; fi
grep -q 'read-only' custom.err
rm devenv.local.nix

for path in relative / /nix /nix/key /run/devenv-secrets /run/devenv-secrets/key /var/../key /var//key /var/./key; do
  printf '{ machines.server.install.secrets."%s".secret = "TOKEN"; }\n' "$path" >devenv.local.nix
  if devenv eval machinesMeta.server.bootstrapSecrets 2>destination.err; then exit 1; fi
  grep -q 'install.secrets: invalid destination' destination.err
done
rm devenv.local.nix

cat >devenv.local.nix <<'LOCAL'
{ config, ... }: {
  machines.wrong.nixos = config.machines.mac.nix-darwin;
}
LOCAL
if devenv eval machinesMeta.wrong.hasNixos 2>wrong.err; then
  echo "expected a nix-darwin configuration in the nixos role to be rejected"
  exit 1
fi
grep -q "expects a nixos configuration, but it is a darwin configuration" wrong.err
rm devenv.local.nix
