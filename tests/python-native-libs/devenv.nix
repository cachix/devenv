{ pkgs, lib, ... }: {
  packages = [ pkgs.cairo ];

  # we must set LD_LIBRARY_PATH by hand without use of the env-venv
  # version of nixpkgs, this can be removed when we switch to one
  env.LD_LIBRARY_PATH = lib.makeLibraryPath [
    pkgs.pythonManylinuxPackages.manylinux2014Package
    pkgs.zlib
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
