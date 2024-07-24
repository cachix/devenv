{ pkgs, lib, ... }: {
  packages = [ pkgs.cairo ];

  languages.python = {
    enable = true;
    directory = "subdir";
    poetry.enable = true;
  };
}
