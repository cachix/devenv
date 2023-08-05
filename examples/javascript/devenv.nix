{ pkgs, lib, ... }:

{
  languages.javascript = {
    enable = true;
    npm.install.enable = true;
  };
}
