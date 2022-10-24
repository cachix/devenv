{ pkgs, ... }:

{
  env.TESTING = 1;

  packages = [ pkgs.git ];

  enterShell = ''
    echo hello from devenv :)
    git --version
  '';
}