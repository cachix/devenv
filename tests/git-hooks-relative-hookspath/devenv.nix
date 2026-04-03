{
  git-hooks.hooks = {
    no-op = {
      enable = true;
      name = "No Op";
      pass_filenames = false;
      raw.always_run = true;
      entry = "true";
    };
  };

  enterTest = ''
    if [ -n "$(git config --get core.hooksPath 2>/dev/null || true)" ]; then
      echo "core.hooksPath should be unset after install"
      exit 1
    fi

    if ! test -f "$(git rev-parse --git-dir)/hooks/pre-commit"; then
      echo "pre-commit hook was not installed"
      exit 1
    fi
  '';
}
