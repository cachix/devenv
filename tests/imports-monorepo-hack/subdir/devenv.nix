{
  # Verify that env.COMMON is set correctly
  enterTest = ''
    if [ -z "$COMMON" ]; then
      echo "COMMON is not set. The ../common/devenv.nix was not loaded correctly."
      exit 1
    fi
  '';
}
