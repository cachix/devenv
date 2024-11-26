{
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
      status = "rm should-not-exist && ls";
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
  '';
}
