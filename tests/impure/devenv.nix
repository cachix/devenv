{
  enterTest = ''
    echo ${builtins.currentSystem};
  '';

  # Test procfile evaluation with --impure
  processes.hello.exec = "echo hello";
}
