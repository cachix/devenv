#!/usr/bin/env bash
set -euo pipefail

devenv machines images list > images.txt
test "$(tail -n +2 images.txt | wc -l)" -eq 4
grep -q $'dev\taarch64-linux\tamazon' images.txt

devenv eval machines.dev.hardware.facter machines.dev.build.nixos machines.dev.build.diskoScript |
  jq -e 'all(.[]; . == null)'

# Both real provider builders evaluate without disko or facter inputs. ARM
# evaluation verifies the image package set without requiring an ARM builder.
devenv eval \
  machines.dev._imageEvals.x86_64-linux.amazon.config.system.build.amazonImage.drvPath \
  machines.dev._imageEvals.x86_64-linux.google-compute.config.system.build.googleComputeImage.drvPath \
  machines.dev._imageEvals.aarch64-linux.amazon.pkgs.stdenv.hostPlatform.system \
  machines.dev._imageEvals.x86_64-linux.amazon.pkgs.imageProviderMarker \
  machines.dev._imageEvals.x86_64-linux.google-compute.pkgs.imageProviderMarker \
  machines.dev._imageEvals.aarch64-linux.google-compute.config.environment.etc.provider-marker.text |
  jq -e '
    .["machines.dev._imageEvals.aarch64-linux.amazon.pkgs.stdenv.hostPlatform.system"] == "aarch64-linux"
    and .["machines.dev._imageEvals.aarch64-linux.google-compute.config.environment.etc.provider-marker.text"] == "google-compute"
    and .["machines.dev._imageEvals.x86_64-linux.amazon.pkgs.imageProviderMarker"] == "amazon"
    and .["machines.dev._imageEvals.x86_64-linux.google-compute.pkgs.imageProviderMarker"] == "google-compute"
    and (.["machines.dev._imageEvals.x86_64-linux.amazon.config.system.build.amazonImage.drvPath"] | endswith(".drv"))
    and (.["machines.dev._imageEvals.x86_64-linux.google-compute.config.system.build.googleComputeImage.drvPath"] | endswith(".drv"))
  '

# The global --system flag filters images without evaluating the outer project
# with a foreign package set. This build needs only the local Linux builder.
devenv machines images build --image amazon --system x86_64-linux > built.txt
test "$(wc -l < built.txt)" -eq 1
artifact=$(cut -f2 built.txt)
test "$(cat "$artifact/system")" = x86_64-linux
test "$(cat "$artifact/format")" = amazon

devenv machines images build --system x86_64-linux dev > built.txt
test "$(wc -l < built.txt)" -eq 2

cat > devenv.local.nix <<'NIX'
{ lib, ... }: { machines.dev.systems = lib.mkForce [ "x86_64-linux" ]; }
NIX
devenv machines images build > built.txt
test "$(wc -l < built.txt)" -eq 2
devenv build machines.dev > aggregate.json
jq -e 'keys == [
  "machines.dev.build.images.x86_64-linux.amazon",
  "machines.dev.build.images.x86_64-linux.google-compute"
]' aggregate.json
rm devenv.local.nix

for args in '--image azure' '--system aarch64-darwin' 'missing'; do
  if devenv machines images build $args >out.log 2>err.log; then
    echo "expected invalid image selection to fail: $args"
    exit 1
  fi
  grep -q 'Unknown' err.log
done

if devenv machines install dev >out.log 2>err.log; then
  echo 'expected image install to be rejected'
  exit 1
fi
grep -q 'launch through the cloud provider' err.log

# A broken unselected format must not prevent a filtered build.
cat > devenv.local.nix <<'NIX'
{ machines.dev.images.google-compute = throw "unselected image was evaluated"; }
NIX
devenv machines images build --image amazon --system x86_64-linux dev > built.txt
rm devenv.local.nix

# Select a concrete deployment combination and inspect the same provider
# configuration used for its image, without contacting the target.
# A local module lets this test vary deployment metadata without rewriting the
# shared matrix definition.
cat > devenv.local.nix <<'NIX'
{
  machines.dev.target = {
    host = "root@image-target.invalid";
    system = "aarch64-linux";
    image = "amazon";
  };
}
NIX
devenv eval \
  machinesMeta.dev.system \
  machines.dev._nixosEval.config.environment.etc.provider-marker.text \
  machines.dev._nixosEval.pkgs.stdenv.hostPlatform.system \
  machines.dev._nixosEval.pkgs.imageProviderMarker |
  jq -e '
    .["machinesMeta.dev.system"] == "aarch64-linux"
    and .["machines.dev._nixosEval.config.environment.etc.provider-marker.text"] == "amazon"
    and .["machines.dev._nixosEval.pkgs.stdenv.hostPlatform.system"] == "aarch64-linux"
    and .["machines.dev._nixosEval.pkgs.imageProviderMarker"] == "amazon"
  '

# Ambiguous deployment selection must fail before any SSH call.
cat > devenv.local.nix <<'NIX'
{ machines.dev.target.host = "root@image-target.invalid"; }
NIX
if devenv machines check dev >out.log 2>err.log; then
  echo 'expected ambiguous deployment selection to fail'
  exit 1
fi
grep -q 'target.system is required' err.log

# Invalid matrix declarations fail during metadata evaluation.
for declaration in \
  'systems = lib.mkForce [];' \
  'systems = lib.mkForce [ "x86_64-linux" "x86_64-linux" ];' \
  'systems = lib.mkForce [ "aarch64-darwin" ];' \
  'images.azure = {};' \
  'nixos = lib.mkForce null;'; do
  printf '{ lib, ... }: { machines.dev = { %s }; }\n' "$declaration" > devenv.local.nix
  if devenv machines images list >out.log 2>err.log; then
    echo "expected invalid matrix declaration to fail: $declaration"
    exit 1
  fi
done

# Single-value axes infer a deployment selection without explicit selectors.
cat > devenv.local.nix <<'NIX'
{ lib, ... }: {
  machines.dev = {
    systems = lib.mkForce [ "x86_64-linux" ];
    images = lib.mkForce { amazon = {}; };
    target.host = "root@image-target.invalid";
  };
}
NIX
devenv eval machines.dev._nixosEval.config.ec2.efi machinesMeta.dev.system |
  jq -e '.["machinesMeta.dev.system"] == "x86_64-linux" and .["machines.dev._nixosEval.config.ec2.efi"] == false'
