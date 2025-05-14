# Assert that .pre-commit-config.yaml is removed when all hooks are disabled
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
    if test -f "$DEVENV_ROOT/.pre-commit-config.yaml"; then
      echo ".pre-commit-config.yaml not removed"
      exit 1
    fi
  '';
}
