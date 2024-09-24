{
  tasks = {
    "myapp:shell".exec = "touch shell";
    "devenv:enterShell".after = [ "myapp:shell" ];
    "myapp:test".exec = "touch test";
    "devenv:enterTest".after = [ "myapp:test" ];
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
  '';
}
