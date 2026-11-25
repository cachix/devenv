#!/usr/bin/env bash
set -e

# First run: link the directory
devenv shell true

test -L src-dir
test -d src-dir
grep -qx "old" src-dir/foo.txt

# Point the link at a different store directory
cat > devenv.nix << 'EOF'
{ pkgs, ... }: {
  files."src-dir".source = pkgs.writeTextDir "foo.txt" "new";

  enterTest = ''
    test -L "$DEVENV_ROOT/src-dir" || { echo "src-dir is not a symlink"; exit 1; }
    grep -qx "new" "$DEVENV_ROOT/src-dir/foo.txt" || { echo "src-dir not updated"; exit 1; }
  '';
}
EOF

# Second run: the link must be replaced, not followed into the old store
# directory. A failure here does not stop the script: `devenv test` runs the
# tasks again and fails if devenv:files cannot update the link.
devenv shell true
