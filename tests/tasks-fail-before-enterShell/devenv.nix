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
}
