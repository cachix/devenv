#!/usr/bin/env bash
set -e

# First run: seed both files from their templates
devenv shell true

# Both files are real writable files, not symlinks into the store
test -f template.txt && test ! -L template.txt
test -f managed.txt && test ! -L managed.txt
test -w template.txt
test -w managed.txt

# Simulate the user editing both files
echo "user edit" > template.txt
echo "user edit" > managed.txt

# Second run re-applies the files
devenv shell true

# Final config asserts the resulting state during `devenv test`
cat > devenv.nix << 'EOF'
{ pkgs, ... }: {
  files."template.txt" = {
    text = "default content\n";
    copy = "copy";
  };
  files."managed.txt" = {
    text = "managed content\n";
    copy = "replace";
  };

  enterTest = ''
    # copy mode: writable regular file, user edit preserved
    test ! -L "$DEVENV_ROOT/template.txt" || { echo "template.txt should not be a symlink"; exit 1; }
    test -w "$DEVENV_ROOT/template.txt" || { echo "template.txt should be writable"; exit 1; }
    grep -qx "user edit" "$DEVENV_ROOT/template.txt" || { echo "template.txt edit not preserved"; exit 1; }

    # replace mode: writable regular file, overwritten back to the template content
    test ! -L "$DEVENV_ROOT/managed.txt" || { echo "managed.txt should not be a symlink"; exit 1; }
    test -w "$DEVENV_ROOT/managed.txt" || { echo "managed.txt should be writable"; exit 1; }
    grep -qx "managed content" "$DEVENV_ROOT/managed.txt" || { echo "managed.txt not replaced"; exit 1; }
  '';
}
EOF
