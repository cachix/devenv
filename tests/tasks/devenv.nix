{ pkgs, ... }: {
  tasks = {
    "myapp:shell" = {
      exec = "touch shell";
      before = [ "devenv:enterShell" ];
    };

    "myapp:test" = {
      exec = "touch test";
    };
    # Test specifying "after"
    "devenv:enterTest".after = [ "myapp:test" ];

    "example:statusIgnored" = {
      before = [ "devenv:enterTest" ];
      exec = "touch ./should-not-exist";
      status = "exit 0";
      # TODO: current broken because `tasks run` will run the full graph.
      # For now, test that status works.
      # status = "rm should-not-exist && ls";
    };

    "test:cwd" = {
      exec = "pwd > $DEVENV_ROOT/cwd-test.txt";
      cwd = "/tmp";
      before = [ "devenv:enterTest" ];
    };
  };

  enterTest = ''
    if [ ! -f shell ]; then
      echo "shell does not exist"
      exit 1
    fi
    rm -f shell
    rm -f test

    devenv tasks run myapp:test >/dev/null
    if [ ! -f test ]; then
      echo "test does not exist"
      exit 1
    fi
    if [ -f ./should-not-exist ]; then
        echo should-not-exist exists
        exit 1
    fi

    # Test cwd functionality
    if [ -f cwd-test.txt ]; then
      CWD_RESULT=$(cat cwd-test.txt)
      # Resolve /tmp to its real path to handle cases where /tmp is a symlink (e.g. macOS)
      CWD_EXPECTED=$(${pkgs.coreutils}/bin/realpath "/tmp")
      if [ "$CWD_RESULT" != "$CWD_EXPECTED" ]; then
        echo "Expected cwd to be $CWD_EXPECTED but got $CWD_RESULT"
        exit 1
      fi
      rm -f cwd-test.txt
    else
      echo "cwd-test.txt not found - test:cwd task did not run"
      exit 1
    fi
  '';
}
