{ pkgs, lib, ... }:

{
  languages.javascript = {
    enable = true;
    directory = "directory";
    package = pkgs.nodejs-slim;
    npm = {
      enable = true;
      install.enable = true;
    };
  };
}
