#!/usr/bin/env bash
set -euo pipefail

mkdir -p mock-bin ssh-state
cat >mock-bin/ssh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

remote="${!#}"
printf '%s\n' "$remote" >>"$MOCK_SSH_LOG"

if [[ "$remote" == *"has_tar="* ]]; then
  printf 'user=0\nhas_tar=1\nhas_curl=1\n'
elif [[ "$remote" == "cat /proc/sys/kernel/random/boot_id" ]]; then
  if [[ -e "$MOCK_SSH_STATE/kexec-started" ]]; then
    count=0
    if [[ -e "$MOCK_SSH_STATE/boot-probes" ]]; then
      count=$(<"$MOCK_SSH_STATE/boot-probes")
    fi
    printf '%s\n' "$((count + 1))" >"$MOCK_SSH_STATE/boot-probes"
    if (( count == 0 )); then
      printf 'old-boot\n'
    else
      printf 'new-boot\n'
    fi
  else
    printf 'old-boot\n'
  fi
elif [[ "$remote" == *"/root/kexec/run"* ]]; then
  touch "$MOCK_SSH_STATE/kexec-started"
elif [[ "$remote" == "nixos-facter" ]]; then
  printf '{"hardware":"ok"}\n'
elif [[ "$remote" == "reboot" ]]; then
  if [[ "${MOCK_REBOOT_FAIL:-}" == 1 ]]; then
    exit 23
  fi
else
  echo "unexpected SSH command: $remote" >&2
  exit 98
fi
EOF
chmod +x mock-bin/ssh

export MOCK_SSH_LOG="$PWD/ssh.log"
export MOCK_SSH_STATE="$PWD/ssh-state"
export PATH="$PWD/mock-bin:$PATH"

# Duplicate names must fail before metadata preparation or target access.
: >"$MOCK_SSH_LOG"
if devenv machines install server server 2>duplicate.err; then
  echo "expected duplicate install names to fail"
  exit 1
fi
if ! grep -q "Duplicate machine name" duplicate.err; then
  echo "duplicate install failed for an unexpected reason"
  cat duplicate.err
  exit 1
fi
if [[ -s "$MOCK_SSH_LOG" ]]; then
  echo "duplicate install names reached SSH"
  exit 1
fi

# nixos-facter writes JSON to stdout without a --json flag.
: >"$MOCK_SSH_LOG"
devenv machines install --phases facter server
jq -e '.hardware == "ok"' .machines/server/facter.json
grep -Fxq "nixos-facter" "$MOCK_SSH_LOG"
if grep -q -- "--json" "$MOCK_SSH_LOG"; then
  echo "nixos-facter was invoked with the unsupported --json flag"
  exit 1
fi

# A successful SSH probe from the old OS must not satisfy kexec readiness.
: >"$MOCK_SSH_LOG"
rm -f ssh-state/kexec-started ssh-state/boot-probes
devenv machines install --phases kexec server
if [[ $(<ssh-state/boot-probes) -lt 2 ]]; then
  echo "kexec readiness accepted the old boot ID"
  exit 1
fi

# A genuine remote reboot failure is not an expected SSH disconnect.
: >"$MOCK_SSH_LOG"
rm -f ssh-state/kexec-started ssh-state/boot-probes
if MOCK_REBOOT_FAIL=1 devenv machines install --phases reboot server 2>reboot.err; then
  echo "expected reboot failure to propagate"
  exit 1
fi
if ! grep -q "did not accept the reboot command" reboot.err; then
  echo "reboot failed for an unexpected reason"
  cat reboot.err
  exit 1
fi

echo "all install control-flow checks passed"
