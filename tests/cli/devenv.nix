{ pkgs, inputs, ... }: {
  languages.python.enable = true;

  tasks."test:export-env" = {
    exec = ''
      export DEVENV_CLI_TEST_VAR="hello-from-task"
    '';
    exports = [ "DEVENV_CLI_TEST_VAR" ];
    before = [ "devenv:enterShell" ];
  };
}
