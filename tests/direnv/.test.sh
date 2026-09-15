# Test the direnv integration
#
# Our main concern is that `devenv shell` should not trigger direnv to immediately reload.
# Because direnv only checks the modification time of watched files, we need to take extra care not to "carelessly" write to such files.
set -xeuo pipefail

# Install direnv
export PATH="$(nix build nixpkgs#direnv --print-out-paths)/bin:$PATH"

export TMPDIR=$(mktemp -d)
export XDG_CONFIG_HOME=${TMPDIR}/config
export XDG_DATA_HOME=${TMPDIR}/data

direnv_eval() {
	eval "$(direnv export bash)"
}

# Setup direnv
mkdir -p $XDG_CONFIG_HOME/.config/direnv/
cat >$XDG_CONFIG_HOME/.config/direnv/direnv.toml <<'EOF'
[global]
strict_env = true
EOF

# Define the devenv arguments
DEVENV_ARGS="--verbose"

# Initialize direnv
cat >.envrc <<EOF
  eval "\$(devenv direnvrc)"
  use devenv $DEVENV_ARGS
EOF

# Load the environment
direnv allow
direnv_eval

# Verify that packages are on PATH via direnv (#2574)
if ! command -v hello &>/dev/null; then
  echo "FAIL: 'hello' package is not on PATH after direnv eval" >&2
  exit 1
fi
echo "PASS: 'hello' package is on PATH via direnv" >&2

# Verify that enterShell tasks ran and exported env vars
if [[ "${DEVENV_DIRENV_TASK_VAR:-}" != "hello-from-direnv-task" ]]; then
	echo "FAIL: DEVENV_DIRENV_TASK_VAR not set by task, got: '${DEVENV_DIRENV_TASK_VAR:-}'" >&2
	exit 1
fi
echo "PASS: enterShell task exported DEVENV_DIRENV_TASK_VAR correctly" >&2

# Enter shell and capture initial watches
DIRENV_WATCHES_BEFORE=$DIRENV_WATCHES

# Verify DEVENV_CMDLINE matches the expected arguments
if [[ "${DEVENV_CMDLINE:-}" != "$DEVENV_ARGS" ]]; then
	echo "FAIL: DEVENV_CMDLINE is not set to '$DEVENV_ARGS', got: '${DEVENV_CMDLINE:-}'" >&2
	exit 1
fi
echo "PASS: DEVENV_CMDLINE is correctly set to: $DEVENV_CMDLINE" >&2

# Execute some operations that should not cause direnv to reload
echo "Running commands that should not trigger direnv reload..." >&2

# Environment capture is one-shot, so repeated captures must not persist their
# activation scripts in the project directory (#3149).
SHELL_SCRIPTS_BEFORE=$(find .devenv -maxdepth 1 -type f -name 'shell-*.sh' | wc -l | tr -d ' ')

for _ in 1 2 3; do
	devenv direnv-export >/dev/null
done

# Capture paths are arguments, not shell source, so unusual temporary-directory
# names must be handled literally.
TMPDIR_WITH_SPACES="$PWD/tmp dir"
mkdir -p "$TMPDIR_WITH_SPACES"
TMPDIR="$TMPDIR_WITH_SPACES" devenv direnv-export >/dev/null

SHELL_SCRIPTS_AFTER=$(find .devenv -maxdepth 1 -type f -name 'shell-*.sh' | wc -l | tr -d ' ')
if [[ "$SHELL_SCRIPTS_BEFORE" != "$SHELL_SCRIPTS_AFTER" ]]; then
	echo "FAIL: direnv-export leaked shell scripts: $SHELL_SCRIPTS_BEFORE -> $SHELL_SCRIPTS_AFTER" >&2
	exit 1
fi
echo "PASS: direnv-export did not persist activation scripts" >&2

SHELL_COMMAND_OUTPUT=$(devenv shell -- printf '<%s>\n' 'Hello from devenv shell')
if [[ "$SHELL_COMMAND_OUTPUT" != '<Hello from devenv shell>' ]]; then
	echo "FAIL: shell command arguments were not preserved: $SHELL_COMMAND_OUTPUT" >&2
	exit 1
fi
if grep -Fq 'Hello from devenv shell' .devenv/shell-*.sh; then
	echo "FAIL: shell command arguments were embedded in the activation script" >&2
	exit 1
fi
echo "PASS: shell command arguments were passed separately" >&2

direnv_eval

# Capture watches after
DIRENV_WATCHES_AFTER=$DIRENV_WATCHES

echo "Checking whether direnv reload was triggered..." >&2
if [[ "$DIRENV_WATCHES_BEFORE" == "$DIRENV_WATCHES_AFTER" ]]; then
	echo "PASS: DIRENV_WATCHES remained unchanged" >&2
	exit 0
else
	echo "FAIL: DIRENV_WATCHES changed, indicating unwanted reload" >&2
	echo "Before: $DIRENV_WATCHES_BEFORE" >&2
	echo "After:  $DIRENV_WATCHES_AFTER" >&2
	exit 1
fi
