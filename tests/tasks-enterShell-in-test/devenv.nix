{ ... }:
{
  # Task that exports an env var before enterShell.
  # This tests that run_enter_shell_tasks() is called during `devenv test`
  # and that exported env vars are available in the test script.
  tasks."test:export-env" = {
    exec = ''
      export DEVENV_TEST_VAR="hello-from-task"
    '';
    exports = [ "DEVENV_TEST_VAR" ];
    before = [ "devenv:enterShell" ];
  };
}
