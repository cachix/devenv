{ pkgs, ... }: {
  packages = with pkgs; [
    pkgs.stdenv.cc.cc.lib
    pkgs.cairo
    pkgs.glibc
  ];

  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      pillow
      grpcio-tools
      transformers
      torch
    '';
  };
}
