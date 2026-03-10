{ pkgs, lib, ... }: {
  packages = [ pkgs.cairo pkgs.zlib ];

  languages.python = {
    enable = true;
    version = "3.12"; # Use Python 3.12 for better wheel availability
    directory = "subdir";
    poetry.enable = true;
  };
}
