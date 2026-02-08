{ pkgs, ... }:

{
  # Task that fails before enterShell
  # This tests that task failures don't block shell entry (non-fatal)
  tasks."test:fail-before-shell" = {
    description = "Task that fails intentionally";
    exec = ''
      echo "This task is failing..."
      exit 1
    '';
    before = [ "devenv:enterShell" ];
  };
}
