{ pkgs, lib, ... }:

{
  languages.javascript = {
    enable = true;
    directory = "directory";
    package = pkgs.nodejs-slim;
    bun = {
      enable = true;
      install.enable = true;
    };
    # npm = {
    #   enable = true;
    #   install.enable = true;
    # };
    corepack.enable = true;
  };
}
