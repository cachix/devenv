{ ... }:
{
  # Task that exports an env var before enterShell.
  # This tests that enterShell tasks run during `devenv test`
  # and that exported env vars are available in the test script.
  tasks."test:export-env" = {
    exec = ''
      export DEVENV_TEST_VAR="hello-from-task"
      export DEVENV_TEST_MULTI="second-var"
      export DEVENV_TEST_EMPTY=""
      export DEVENV_TEST_SPACES="hello world with spaces"
      export DEVENV_TEST_EQUALS="key=value=more"
      export DEVENV_TEST_NOT_EXPORTED="should-not-leak"
    '';
    exports = [
      "DEVENV_TEST_VAR"
      "DEVENV_TEST_MULTI"
      "DEVENV_TEST_EMPTY"
      "DEVENV_TEST_SPACES"
      "DEVENV_TEST_EQUALS"
    ];
    before = [ "devenv:enterShell" ];
  };

  # Second task exporting vars, tests that exports from multiple tasks merge.
  tasks."test:export-env-2" = {
    exec = ''
      export DEVENV_TEST_FROM_SECOND="from-second-task"
    '';
    exports = [ "DEVENV_TEST_FROM_SECOND" ];
    before = [ "devenv:enterShell" ];
  };

  # Task that runs before enterTest (but not enterShell).
  # This tests that `devenv test` runs the enterTest task root,
  # not just enterShell tasks.
  tasks."test:enter-test-only" = {
    exec = ''
      export DEVENV_TEST_ENTER_TEST_RAN="yes"
    '';
    exports = [ "DEVENV_TEST_ENTER_TEST_RAN" ];
    before = [ "devenv:enterTest" ];
  };
}
