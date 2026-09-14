{ pkgs, inputs, ... }:
{
  languages.python.enable = true;

  # A name a command wrapper could shadow.
  env.command = "env-command";

  tasks."test:export-env" = {
    exec = ''
      export DEVENV_CLI_TEST_VAR="hello-from-task"
    '';
    exports = [ "DEVENV_CLI_TEST_VAR" ];
    before = [ "devenv:enterShell" ];
  };
}
