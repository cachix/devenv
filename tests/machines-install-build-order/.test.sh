#!/usr/bin/env bash
set -euo pipefail

mkdir -p mock-bin
export INSTALL_LOG="$PWD/install.log"
export REAL_NIX
REAL_NIX=$(command -v nix)
cat >mock-bin/ssh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'ssh %s\n' "${!#}" >>"$INSTALL_LOG"
EOF
cat >mock-bin/nix <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == copy ]]; then
  printf 'copy %s\n' "${!#}" >>"$INSTALL_LOG"
else
  exec "$REAL_NIX" "$@"
fi
EOF
chmod +x mock-bin/*
export PATH="$PWD/mock-bin:$PATH"

# A real failed derivation must prevent both disko transfer and execution.
: >"$INSTALL_LOG"
if devenv machines install failing --phases disko,install >failure.log 2>&1; then
  echo "expected the replacement system build to fail"
  exit 1
fi
if ! grep -q 'intentional replacement build failure' failure.log; then
  cat failure.log
  exit 1
fi
if [[ -s "$INSTALL_LOG" ]]; then
  echo "installation touched the target before the replacement built"
  cat "$INSTALL_LOG"
  exit 1
fi

# Successful installation copies and installs precisely the built output.
devenv build machines.working.build.nixos >built.json
system=$(jq -er '."machines.working.build.nixos"' built.json)
devenv machines install working --phases disko,install
grep -Fxq "copy $system" "$INSTALL_LOG"
grep -Fxq "ssh nixos-install --system $system --no-root-password --no-channel-copy" "$INSTALL_LOG"

# An explicitly disk-only operation must not build an unselected system.
: >"$INSTALL_LOG"
devenv machines install failing --phases disko
grep -q 'ssh .*mock-disko' "$INSTALL_LOG"
if grep -q nixos-install "$INSTALL_LOG"; then
  echo "disk-only operation unexpectedly installed a system"
  exit 1
fi
