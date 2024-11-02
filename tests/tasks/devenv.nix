{
  tasks = {
    "myapp:shell".exec = "touch shell";
    "devenv:enterShell".after = [ "myapp:shell" ];
    "myapp:test".exec = "touch test";
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
