{ config, ... }:

{
  # Verify that env.COMMON is set correctly
  enterTest = ''
    if [ -z "$COMMON" ]; then
      echo "COMMON is not set. The /tests/imports-monorepo-hack/common/devenv.nix was not loaded correctly."
      exit 1
    fi

    # Wait for the process to write the cwd file
    sleep 1

    # Check if the process wrote the correct working directory
    if [ -f "$DEVENV_STATE/process-cwd.txt" ]; then
      PROCESS_CWD=$(cat "$DEVENV_STATE/process-cwd.txt")
      EXPECTED_CWD="${config.git.root}/tests/imports-monorepo-hack/subdir"
      
      if [ "$PROCESS_CWD" = "$EXPECTED_CWD" ]; then
        echo "SUCCESS: Process is running from the correct directory"
        echo "Process CWD: $PROCESS_CWD"
      else
        echo "ERROR: Process is not running from the expected directory"
        echo "Expected: $EXPECTED_CWD"
        echo "Actual: $PROCESS_CWD"
        exit 1
      fi
    else
      echo "ERROR: Process did not write the cwd file"
      exit 1
    fi
  '';

  # Test process that runs from a specific subdirectory relative to git root
  processes = {
    test-git-root = {
      exec = ''
        pwd > $DEVENV_STATE/process-cwd.txt
      '';
      cwd = "${config.git.root}/tests/imports-monorepo-hack/subdir";
    };
  };
}
