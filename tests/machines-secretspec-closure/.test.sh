#!/usr/bin/env bash
set -euo pipefail

# A deterministic root-authentication failure must be reported before disko
# can build, copy, or execute its destructive script.
mkdir -p mock-bin
cat >mock-bin/ssh <<'EOF'
#!/usr/bin/env bash
touch "$SSH_CALLED"
exit 97
EOF
chmod +x mock-bin/ssh
if env SSH_CALLED="$PWD/ssh-called" PATH="$PWD/mock-bin:$PATH" \
  devenv machines install --phases disko,install unauthenticated 2>unauthenticated.err; then
  echo "expected unauthenticated install to be rejected"
  exit 1
fi
grep -q "refusing to install" unauthenticated.err
if [[ -e ssh-called ]]; then
  echo "unauthenticated install reached SSH before its root-authentication check"
  exit 1
fi

# NixOS lock markers are password sentinels, not usable credentials.
if env SSH_CALLED="$PWD/ssh-called" PATH="$PWD/mock-bin:$PATH" \
  devenv machines install --phases install locked-root 2>locked-root.err; then
  echo "expected locked root install to be rejected"
  exit 1
fi
grep -q "refusing to install" locked-root.err
if [[ -e ssh-called ]]; then
  echo "locked root install reached SSH before its authentication check"
  exit 1
fi

if env SSH_CALLED="$PWD/ssh-called" PATH="$PWD/mock-bin:$PATH" \
  devenv machines install --phases install locked-initial-root 2>locked-initial-root.err; then
  echo "expected initially locked root install to be rejected"
  exit 1
fi
grep -q "refusing to install" locked-initial-root.err
if [[ -e ssh-called ]]; then
  echo "initially locked root install reached SSH before its authentication check"
  exit 1
fi

build_json=$(devenv build machines.resolver.build.nixos)
toplevel=$(jq -er '."machines.resolver.build.nixos"' <<<"$build_json")
launcher="$toplevel/sw/bin/devenv-machines-secretspec"

if [[ ! -x "$launcher" ]]; then
  echo "target NixOS closure does not contain the SecretSpec launcher: $launcher"
  exit 1
fi

launcher_path=$(readlink -f "$launcher")
if ! grep -q -- '-devenv-bundled-secretspec/bin/secretspec' "$launcher_path"; then
  echo "target launcher does not reference devenv's bundled SecretSpec"
  cat "$launcher_path"
  exit 1
fi

expected_version=$(secretspec --version)
actual_version=$("$launcher" --version)
if [[ "$actual_version" != "$expected_version" ]]; then
  echo "target resolver version differs from the SecretSpec shipped with devenv"
  echo "expected: $expected_version"
  echo "actual:   $actual_version"
  exit 1
fi

echo "built $toplevel with $actual_version"
