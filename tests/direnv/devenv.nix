{ pkgs, ... }:
{
  # Verify that packages are on PATH via direnv (#2574)
  packages = [ pkgs.hello ];

  tasks."test:direnv-export" = {
    exec = ''
      export DEVENV_DIRENV_TASK_VAR="hello-from-direnv-task"
    '';
    exports = [ "DEVENV_DIRENV_TASK_VAR" ];
    before = [ "devenv:enterShell" ];
  };
}
