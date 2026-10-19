{ config, pkgs, ... }:

{
  languages.dang.enable = true;

  assertions = [
    {
      assertion = !(pkgs ? dang);
      message = "This fixture should exercise the bundled Dang package without an overlay.";
    }
    {
      assertion = builtins.elem config.languages.dang.package config.packages;
      message = "languages.dang.enable should add its package to packages.";
    }
  ];

  enterTest = ''
    dang --help
  '';
}
