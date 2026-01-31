#!/usr/bin/env bash
set -e

# First run: create files with initial config
devenv shell true

# Verify initial files were created
test -L a.txt
test -L b.txt
test -L subdir/nested.txt
test -d subdir

# Verify state was saved
test -f .devenv/state/files.json

# Now modify config to remove b.txt and subdir/nested.txt
cat > devenv.nix << 'EOF'
{ pkgs, ... }: {
  files."a.txt".text = "a";

  # Verify cleanup happened
  enterTest = ''
    # a.txt should still exist
    test -L "$DEVENV_ROOT/a.txt" || { echo "a.txt missing"; exit 1; }

    # b.txt should be removed (was in previous config)
    test ! -e "$DEVENV_ROOT/b.txt" || { echo "b.txt not cleaned up"; exit 1; }

    # subdir/nested.txt should be removed
    test ! -e "$DEVENV_ROOT/subdir/nested.txt" || { echo "subdir/nested.txt not cleaned up"; exit 1; }

    # subdir should be removed (empty after cleanup)
    test ! -d "$DEVENV_ROOT/subdir" || { echo "subdir not cleaned up"; exit 1; }

    # State should track only a.txt
    jq -e '.managedFiles | length == 1' "$DEVENV_ROOT/.devenv/state/files.json"
  '';
}
EOF
