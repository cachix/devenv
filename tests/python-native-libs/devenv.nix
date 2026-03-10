{ pkgs, lib, ... }: {
  packages = [ pkgs.cairo pkgs.zlib ];

  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      pillow
      grpcio-tools
      transformers
    '';
  };
}
