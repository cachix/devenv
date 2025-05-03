# Assert that the pre-commit package does not leak its dependencies into the environment.
{
  git-hooks.hooks.nixfmt-rfc-style.enable = true;

  enterTest = ''
    if [ -n "''${PYTHONPATH:-}" ]; then
      echo "PYTHONPATH is non-empty: $PYTHONPATH" >&2
      echo "The pre-commit package is leaking its dependencies into the environment." >&2
      exit 1
    fi
  '';
}
