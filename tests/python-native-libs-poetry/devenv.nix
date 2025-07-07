{ pkgs, lib, ... }:
{
  packages = [ pkgs.cairo ];

  languages.python = {
    enable = true;
    version = "3.12"; # Use Python 3.12 for better wheel availability
    directory = "subdir";
    poetry.enable = true;
  };
}
