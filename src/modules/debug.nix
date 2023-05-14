{ lib, ... }:
{
  options.devenv = {
    debug = lib.mkEnableOption "debug mode of devenv enterShell script";
  };
}
