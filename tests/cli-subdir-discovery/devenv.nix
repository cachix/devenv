{ pkgs, ... }:
{
  packages = [ pkgs.hello ];
  enterShell = ''
    pwd -P > "$DEVENV_ROOT/entershell-pwd"
  '';
}
