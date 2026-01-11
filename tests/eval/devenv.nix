{ pkgs, ... }: {
  languages.python.enable = true;

  env.TEST_VAR = "hello";

  enterShell = ''
    echo "Welcome to the shell"
  '';
}
