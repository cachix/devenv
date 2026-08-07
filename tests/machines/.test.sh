#!/usr/bin/env bash
set -euo pipefail

# SecretSpec's env provider materializes this as an `as_path` temporary file.
# The bootstrap implementation must stream the file contents, not its path.
export SECRET_MACHINE_AGE_KEY="bootstrap-test-value"

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
for m in empty hm mac missing-secret nixos nohost secretful; do
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

# 26. SecretSpec bootstrap metadata exposes only its reference and file
#     metadata, never the secret value itself.
devenv eval 'machinesMeta.secretful.secrets' \
  | jq -e '.["machinesMeta.secretful.secrets"] == [
      {
        "mode": "0644",
        "owner": "0:0",
        "secret": "SECRET_MACHINE_AGE_KEY",
        "target": "/var/lib/sops-nix/key.txt"
      }
    ]'

# 27. A machine install withholds resolved values from Nix (the fixture's mode
#     therefore becomes the valid 0600 instead of its normal-eval 0644), and
#     its as_path value is materialized before the install build begins. The
#     fixture still lacks disko, so that is the next expected failure.
if devenv machines install --phases install secretful 2>err.log; then
  echo "expected missing-disko failure after SecretSpec validation"
  exit 1
fi
grep -q "devenv inputs add disko" err.log
if grep -q "Failed to read temporary file for SecretSpec secret" err.log; then
  echo "as_path SecretSpec bootstrap materialization failed"
  cat err.log
  exit 1
fi

# 28. An undeclared bootstrap reference fails before build, SSH, or destructive
#     work even though SecretSpec itself is enabled and another value resolved.
if devenv machines install --phases install missing-secret 2>err.log; then
  echo "expected unresolved SecretSpec reference to fail"
  exit 1
fi
grep -q "SecretSpec did not resolve 1 bootstrap secret reference" err.log
if grep -q "devenv inputs add disko" err.log; then
  echo "unresolved bootstrap reference was checked after the build"
  cat err.log
  exit 1
fi

# 29. Secret-bearing installs force strict host authentication from their
#     first SSH call, even if target.sshOpts tries to disable it. The mock
#     fails preflight before kexec or any destructive operation.
mkdir -p mock-bin
cat >mock-bin/ssh <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$SSH_ARGS_LOG"
exit 1
EOF
chmod +x mock-bin/ssh
if env SSH_ARGS_LOG="$PWD/ssh-args.log" PATH="$PWD/mock-bin:$PATH" \
  devenv machines install --phases kexec,install secretful 2>err.log; then
  echo "expected mocked strict-host preflight to fail"
  exit 1
fi
if ! grep -q "Preflight probe failed" err.log; then
  echo "mocked SSH did not fail during preflight"
  cat err.log
  exit 1
fi
if test "$(sed -n '1p' ssh-args.log)" != "-o" || \
  test "$(sed -n '2p' ssh-args.log)" != "StrictHostKeyChecking=yes"; then
  echo "strict host checking was not the first effective SSH option"
  cat ssh-args.log
  exit 1
fi
if ! grep -qx "StrictHostKeyChecking=no" ssh-args.log; then
  echo "fixture's insecure SSH option was not present behind the forced policy"
  cat ssh-args.log
  exit 1
fi
if grep -q "bootstrap-test-value" ssh-args.log; then
  echo "secret value leaked into SSH arguments"
  cat ssh-args.log
  exit 1
fi

# 30. Target-side resolution exposes its execution metadata but never inherits
#     or asks the workstation provider for REMOTE_MACHINE_TOKEN. Manifest
#     validation runs first; missing disko is therefore the next failure.
devenv eval 'machinesMeta.target-secretful.secretspec' \
  | jq -e '.["machinesMeta.target-secretful.secretspec"] == {
      "execution": "target",
      "profile": "production",
      "provider": null
    }'
if env -u REMOTE_MACHINE_TOKEN \
  devenv --secretspec-provider env --secretspec-profile production \
    machines install --phases install target-secretful 2>err.log; then
  echo "expected missing-disko failure after target manifest validation"
  exit 1
fi
grep -q "devenv inputs add disko" err.log
if grep -q "REMOTE_MACHINE_TOKEN.*missing\|Missing.*REMOTE_MACHINE_TOKEN" err.log; then
  echo "target-side secret was incorrectly requested from the workstation provider"
  cat err.log
  exit 1
fi

# 31. A target-only install does not require the workstation SecretSpec
#     integration to be enabled. Keep the fixture change scoped to this check.
cp devenv.yaml devenv.yaml.enabled
trap 'mv devenv.yaml.enabled devenv.yaml' EXIT
sed -i 's/  enable: true/  enable: false/' devenv.yaml
if env -u REMOTE_MACHINE_TOKEN \
  devenv machines install --phases install target-secretful 2>err.log; then
  echo "expected missing-disko failure with local SecretSpec disabled"
  exit 1
fi
grep -q "devenv inputs add disko" err.log
if grep -q "require an enabled secretspec\|Machine bootstrap secrets require SecretSpec" err.log; then
  echo "target-only bootstrap incorrectly required local SecretSpec"
  cat err.log
  exit 1
fi
mv devenv.yaml.enabled devenv.yaml
trap - EXIT

echo "all machines checks passed"
