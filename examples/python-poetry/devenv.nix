{ pkgs, ... }:

{
  packages = [ pkgs.zlib ];
  languages.python = {
    enable = true;
    poetry.enable = true;
  };
}
