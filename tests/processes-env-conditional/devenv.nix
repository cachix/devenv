# Test for https://github.com/cachix/devenv/issues/2278
# Using config.env.* within processes.* should not cause infinite recursion.
{ lib, config, ... }:

{
  env.ENABLE_PROCESS = "true";

  # This pattern caused infinite recursion before the fix
  processes = lib.mkIf (config.env.ENABLE_PROCESS == "true") {
    greet.exec = "echo hello";
  };

  enterTest = ''
    # Verify the process was defined
    if [ -z "''${ENABLE_PROCESS:-}" ]; then
      echo "ENABLE_PROCESS env var not set"
      exit 1
    fi
    echo "Test passed: config.env.* can be used in processes conditional"
  '';
}
