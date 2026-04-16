#!/usr/bin/env bash
set -euo pipefail

# 1. Schema evaluates and an empty machine has the expected default system.
devenv eval 'machines.empty.system' | jq -e '.["machines.empty.system"] == "x86_64-linux"'

# 2. A machine with no roles has all build.* outputs null.
devenv eval 'machines.empty.build.nixos' | jq -e '.["machines.empty.build.nixos"] == null'
devenv eval 'machines.empty.build.nix-darwin' | jq -e '.["machines.empty.build.nix-darwin"] == null'
devenv eval 'machines.empty.build.home-manager' | jq -e '.["machines.empty.build.home-manager"] == null'

# 3. A NixOS-using machine without `disko` as an input fails with the
#    targeted "devenv inputs add disko" hint when `build.nixos` is forced,
#    but `.system` and `.target.host` still evaluate (errors are lazy).
devenv eval 'machines.nixos.system' | jq -e '.["machines.nixos.system"] == "x86_64-linux"'
devenv eval 'machines.nixos.target.host' | jq -e '.["machines.nixos.target.host"] == "root@host"'

if devenv eval 'machines.nixos.build.nixos' 2>err.log; then
  echo "expected missing-disko error, but eval succeeded"
  exit 1
fi
# miette wraps error output across terminal lines, so match a short unique
# substring that can't straddle a wrap boundary.
grep -q "devenv inputs add disko" err.log

# 4. A home-manager-only machine without `home-manager` as an input fails with
#    the home-manager-specific hint, NOT the disko hint — the nixos branch
#    must stay lazy.
if devenv eval 'machines.hm.build.home-manager' 2>err.log; then
  echo "expected missing-home-manager error, but eval succeeded"
  exit 1
fi
grep -q "devenv inputs add home-manager" err.log
if grep -q "devenv inputs add disko" err.log; then
  echo "disko error leaked into a home-manager-only machine"
  exit 1
fi

# 5. The walker extension: `devenv build machines.empty` must succeed even
#    though every role on the empty machine is null. A machine with no roles
#    produces no build paths, not an error.
devenv build machines.empty

# 6. `devenv machines deploy <unknown>` errors before touching any target,
#    listing the machines that *are* defined. Validates that machinesMeta
#    loads and that validation fires before any SSH call.
if devenv machines deploy bogus 2>err.log; then
  echo "expected unknown-machine error, but deploy succeeded"
  exit 1
fi
grep -q "Unknown machine" err.log
grep -q "empty" err.log

# 7. A NixOS machine without target.host errors with a targeted message
#    rather than trying to connect to an empty destination.
if devenv machines deploy nohost 2>err.log; then
  echo "expected missing-target.host error, but deploy succeeded"
  exit 1
fi
grep -q "requires target.host" err.log

# 8. A nix-darwin machine without target.host errors with its own targeted
#    message. (nix-darwin deploys are implemented but always require SSH.)
if devenv machines deploy mac 2>err.log; then
  echo "expected nix-darwin missing-target.host error, but deploy succeeded"
  exit 1
fi
grep -q "machines.mac.nix-darwin requires target.host" err.log

# 9. Bulk `devenv machines deploy` (no names) enumerates the whole attrset
#    via machinesMeta. On this fixture:
#    - `nixos` has target.host = root@host → is picked up by bulk and fails
#      at the build step (disko is not an input of the test environment).
#    - `empty`, `hm`, `mac`, `nohost` → have no target.host and are silently
#      skipped via activity.skipped() instead of being attempted.
#    We expect exit != 0 with `nixos` listed in the failure details and no
#    deploy attempted on the targetless entries (if one were attempted, the
#    home-manager input-missing error would also leak into bulk.log).
if devenv machines deploy >bulk.log 2>&1; then
  echo "expected bulk deploy to fail on missing disko, but it succeeded"
  cat bulk.log
  exit 1
fi
grep -q "nixos" bulk.log
grep -q "devenv inputs add disko" bulk.log
if grep -q "devenv inputs add home-manager" bulk.log; then
  echo "bulk deploy attempted targetless home-manager machine"
  cat bulk.log
  exit 1
fi

# 10. `--max-concurrent 1` is accepted and forces sequential deploys. Same
#     fixture, same exit code, same disko hint — the flag must not change
#     behaviour beyond limiting parallelism.
if devenv machines deploy --max-concurrent 1 >seq.log 2>&1; then
  echo "expected --max-concurrent 1 deploy to fail on missing disko, but it succeeded"
  cat seq.log
  exit 1
fi
grep -q "devenv inputs add disko" seq.log

# 11. Bare `--max-concurrent` without a value is a parse error.
if devenv machines deploy --max-concurrent 2>/dev/null; then
  echo "expected --max-concurrent with no value to fail"
  exit 1
fi

# 12. `devenv machines info` with no names prints every machine. The table
#     output uses ANSI escapes, so strip them before matching. Each machine
#     name must appear at least once.
devenv machines info >info.log 2>&1
sed 's/\x1b\[[0-9;]*m//g' info.log >info.stripped
for m in empty hm mac nixos nohost; do
  grep -q "^| $m" info.stripped || (echo "missing $m in info output"; cat info.stripped; exit 1)
done

# 13. `devenv machines info <names>` restricts to the named machines, and
#     rejects unknown names with the same error shape as deploy.
devenv machines info nixos hm >info2.log 2>&1
sed 's/\x1b\[[0-9;]*m//g' info2.log >info2.stripped
grep -q "^| nixos" info2.stripped
grep -q "^| hm" info2.stripped
if grep -q "^| empty" info2.stripped; then
  echo "machines info nixos hm listed 'empty' when it shouldn't have"
  exit 1
fi

if devenv machines info bogus 2>info3.err; then
  echo "expected unknown-machine error on info"
  exit 1
fi
grep -q "Unknown machine" info3.err

# 14. `devenv machines install` requires explicit names (disk-wiping safety).
if devenv machines install 2>/dev/null; then
  echo "expected install with no args to fail"
  exit 1
fi

# 15. Install rejects unknown machines, same error shape as deploy.
if devenv machines install bogus 2>err.log; then
  echo "expected unknown-machine error, but install succeeded"
  exit 1
fi
grep -q "Unknown machine" err.log

# 16. Install rejects non-NixOS machines (home-manager-only).
if devenv machines install hm 2>err.log; then
  echo "expected non-nixos error, but install succeeded"
  exit 1
fi
grep -q "does not have a .nixos. module" err.log

# 17. Install rejects NixOS machines without target.host.
if devenv machines install nohost 2>err.log; then
  echo "expected missing-target.host error, but install succeeded"
  exit 1
fi
grep -q "does not have .target.host. set" err.log

# 18. Install on a valid NixOS+target machine passes validation and reaches
#     the preflight probe, which fails because root@host is unreachable.
#     This proves all pre-flight validation passed and the pipeline started.
if devenv machines install nixos 2>err.log; then
  echo "expected install to fail at preflight (unreachable host), but it succeeded"
  exit 1
fi
grep -q "Preflight probe failed" err.log

# 19. The install.kexec.{image, postSshPort} schema evaluates and surfaces
#     in machinesMeta.
devenv eval 'machinesMeta.custom-kexec.kexecImage' \
  | jq -e '.["machinesMeta.custom-kexec.kexecImage"] == "https://example.com/custom-kexec.tar.gz"'
devenv eval 'machinesMeta.custom-kexec.kexecPostSshPort' \
  | jq -e '.["machinesMeta.custom-kexec.kexecPostSshPort"] == 2222'

# 21. install.copyHostKeys evaluates in machinesMeta.
devenv eval 'machinesMeta.custom-kexec.copyHostKeys' \
  | jq -e '.["machinesMeta.custom-kexec.copyHostKeys"] == true'

# 22. `--phases` with a valid subset is accepted. Same SSH failure as test 18.
if devenv machines install --phases install,reboot nixos 2>err.log; then
  echo "expected install with --phases to fail (unreachable host)"
  exit 1
fi
# Should reach the install phase (skips kexec/facter/disko), which tries
# to build the NixOS toplevel. Without disko input, build fails.
grep -q "devenv inputs add disko" err.log

# 23. `--stop-after-disko` is accepted and conflicts with `--phases`.
if devenv machines install --stop-after-disko --phases kexec nixos 2>/dev/null; then
  echo "expected conflict error between --stop-after-disko and --phases"
  exit 1
fi

# 24. `--disko-mode format` is accepted.
if devenv machines install --disko-mode format nixos 2>err.log; then
  echo "expected install to fail (unreachable host)"
  exit 1
fi
grep -q "Preflight probe failed" err.log

# 25. `--use-machines-as-builders` is accepted on both deploy and install.
if devenv machines deploy --use-machines-as-builders bogus 2>err.log; then
  echo "expected unknown-machine error on deploy with --use-machines-as-builders"
  exit 1
fi
grep -q "Unknown machine" err.log

if devenv machines install --use-machines-as-builders nohost 2>err.log; then
  echo "expected missing-target error on install with --use-machines-as-builders"
  exit 1
fi
grep -q "does not have .target.host. set" err.log

echo "all machines checks passed"
