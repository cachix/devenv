{ pkgs, config, ... }:

{
  languages.python = {
    enable = true;
    directory = "./directory";
    poetry = {
      enable = true;
      install.enable = true;
      activate.enable = true;
    };
  };
}
