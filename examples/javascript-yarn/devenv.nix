{ pkgs, lib, ... }:

{
  languages.javascript = {
    enable = true;
    yarn = {
      enable = true;
      install.enable = true;
    };
  };
}
