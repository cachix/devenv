{ pkgs, ... }:
{
  packages = [
    pkgs.python3
    pkgs.git
  ];
  env.PROJECT_GREETING = "Hello from the prebaked devenv environment";
  enterShell = ''
    export PROJECT_SHELL_READY=1
    echo "$PROJECT_GREETING"
  '';
}
