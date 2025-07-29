{
  # Verify that env.COMMON is set correctly
  enterTest = ''
    if [ -z "$COMMON" ]; then
      echo "COMMON is not set. The /tests/imports-monorepo-hack/common/devenv.nix was not loaded correctly."
      exit 1
    fi
  '';
}
