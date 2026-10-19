{ config, pkgs, ... }:

{
  dagger = {
    enable = true;
    package = pkgs.hello;
    dang.enable = false;
  };
  languages.dang.enable = true;

  assertions = [
    {
      assertion = config.languages.dang.enable;
      message = "Dang should remain independently configurable when dagger.dang.enable is false.";
    }
    {
      assertion = builtins.elem config.languages.dang.package config.packages;
      message = "Independently enabled Dang should be installed.";
    }
  ];
}
