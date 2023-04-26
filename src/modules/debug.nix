{ lib, ... }:
{
  options.devenv = {
    debug = lib.mkEnableOption "debug mode of devenv enterShell script";
  };
  config = {
    enterShell = lib.mkOrder 100 ''
      set -x
    '';
  };
}
