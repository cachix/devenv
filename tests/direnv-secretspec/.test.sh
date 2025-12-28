# Test that secretspec errors correctly with direnv (non-TUI mode)
#
# This test verifies that:
# 1. When secrets are missing, devenv errors instead of prompting
# 2. The error message tells user to run devenv shell interactively
set -xeuo pipefail

# Use the local devenv build (cargo target or provided DEVENV_BIN)
DEVENV_BIN="${DEVENV_BIN:-../../target/debug/devenv}"
if [ ! -x "$DEVENV_BIN" ]; then
  echo "Error: devenv binary not found at $DEVENV_BIN. Run 'cargo build' first." >&2
  exit 1
fi
export PATH="$(dirname "$(realpath "$DEVENV_BIN")"):$PATH"

# Install direnv
export PATH="$(nix build nixpkgs#direnv --print-out-paths)/bin:$PATH"

export TMPDIR=$(mktemp -d)
export XDG_CONFIG_HOME=${TMPDIR}/config
export XDG_DATA_HOME=${TMPDIR}/data

# Setup direnv
mkdir -p $XDG_CONFIG_HOME/.config/direnv/
cat > $XDG_CONFIG_HOME/.config/direnv/direnv.toml << 'EOF'
[global]
strict_env = true
EOF

# Initialize direnv (without the .env file so secrets are missing)
rm -f .env
cat > .envrc << 'EOF'
  eval "$(devenv direnvrc)"
  use devenv
EOF

# Try to load the environment - should fail with missing secrets error
direnv allow

echo "Testing that missing secrets causes an error (not a prompt)..." >&2
output=$(direnv export bash 2>&1) || true

# Check that no TUI escape codes are in the output (no prompting should happen)
if echo "$output" | grep -q '\[?25l\|\[?2026h'; then
  echo "FAIL: TUI escape codes found in output - TUI should not run in direnv" >&2
  echo "Output was: $output" >&2
  exit 1
fi
echo "PASS: No TUI escape codes in output" >&2

# Check that the error message mentions missing secrets
if echo "$output" | grep -q "Missing required secrets"; then
  echo "PASS: Error message correctly mentions missing secrets" >&2
else
  echo "FAIL: Error message should mention missing secrets" >&2
  echo "Output was: $output" >&2
  exit 1
fi

# Check that the error message tells user to run devenv shell interactively
if echo "$output" | grep -q "devenv shell"; then
  echo "PASS: Error message correctly tells user to run devenv shell" >&2
else
  echo "FAIL: Error message should mention running devenv shell" >&2
  echo "Output was: $output" >&2
  exit 1
fi

echo "PASS: direnv-secretspec error handling test passed" >&2
