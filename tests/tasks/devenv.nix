{
  tasks = {
    shell.exec = "touch shell";
    enterShell.depends = [ "shell" ];
    test.exec = "touch test";
  };

  enterTest = ''
    if [ ! -f shell ]; then
      echo "shell does not exist"
      exit 1
    fi
    devenv tasks run test
    if [ ! -f test ]; then
      echo "test does not exist"
      exit 1
    fi
  '';
}
