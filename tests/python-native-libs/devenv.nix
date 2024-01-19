{ pkgs, ... }: {
  # this test fails without use of the env-venv version of nixpkgs
  packages = [ pkgs.cairo ];

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
