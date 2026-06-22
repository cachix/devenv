{ pkgs, ... }: {
  files."template.txt" = {
    text = "default content\n";
    copyMode = "seed";
  };
  files."managed.txt" = {
    text = "managed content\n";
    copyMode = "copy";
  };

  enterTest = ''
    # seed mode: writable regular file, user edit preserved
    test ! -L "$DEVENV_ROOT/template.txt" || { echo "template.txt should not be a symlink"; exit 1; }
    test -w "$DEVENV_ROOT/template.txt" || { echo "template.txt should be writable"; exit 1; }
    grep -qx "user edit" "$DEVENV_ROOT/template.txt" || { echo "template.txt edit not preserved"; exit 1; }

    # copy mode: writable regular file, overwritten back to the template content
    test ! -L "$DEVENV_ROOT/managed.txt" || { echo "managed.txt should not be a symlink"; exit 1; }
    test -w "$DEVENV_ROOT/managed.txt" || { echo "managed.txt should be writable"; exit 1; }
    grep -qx "managed content" "$DEVENV_ROOT/managed.txt" || { echo "managed.txt not overwritten"; exit 1; }
  '';
}
