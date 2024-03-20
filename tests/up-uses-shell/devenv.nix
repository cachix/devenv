{
  enterShell = ''
    export FOO=1
  '';

  enterTest = ''
    sleep 1
    if [ $(cat foo) -ne 1 ]; then
      exit 1
    fi
  '';

  processes.test.exec = "while true; do echo $FOO > foo; sleep 1; done";
}
