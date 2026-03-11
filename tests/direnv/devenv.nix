{ ... }:
{
  tasks."test:direnv-export" = {
    exec = ''
      export DEVENV_DIRENV_TASK_VAR="hello-from-direnv-task"
    '';
    exports = [ "DEVENV_DIRENV_TASK_VAR" ];
    before = [ "devenv:enterShell" ];
  };
}
