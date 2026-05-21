{ pkgs, ... }:

{
  packages = [ pkgs.hello ];

  enterShell = ''
    echo DEVENV_ENTER_OK
  '';
}
