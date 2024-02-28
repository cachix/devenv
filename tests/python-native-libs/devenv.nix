{ pkgs, lib, ... }: {
  packages = [ pkgs.cairo ];

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
