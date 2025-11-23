{ pkgs, ... }:

{
  # Task that should fail before enterShell
  tasks."test:fail-before-shell" = {
    description = "Task that fails intentionally";
    exec = ''
      echo "This task is failing..."
      exit 1
    '';
    before = [ "devenv:enterShell" ];
  };

  # This should NOT run because the dependency task fails
  enterShell = ''
    echo "SHELL_ENTERED" >> /tmp/devenv-test-shell-entered.txt
  '';

  enterTest = ''
    # Clean up
    rm -f /tmp/devenv-test-shell-entered.txt

    # Try to enter the shell (this should fail and not create the marker file)
    if devenv shell --quiet 2>&1; then
      # Shell entered successfully - this is the bug!
      if [ -f /tmp/devenv-test-shell-entered.txt ]; then
        echo "✗ BUG: Shell entered even though dependency task failed"
        cat /tmp/devenv-test-shell-entered.txt
        exit 1
      else
        echo "✗ Shell command succeeded but our enterShell didn't run"
        exit 1
      fi
    else
      # Shell failed to enter - this is expected
      if [ -f /tmp/devenv-test-shell-entered.txt ]; then
        echo "✗ Shell command failed but enterShell still ran"
        cat /tmp/devenv-test-shell-entered.txt
        exit 1
      else
        echo "✓ Shell correctly failed to enter when dependency task failed"
      fi
    fi

    # Clean up
    rm -f /tmp/devenv-test-shell-entered.txt
  '';
}
