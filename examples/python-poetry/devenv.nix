{ pkgs, lib, config, ... }:

{
  packages = [
    # A python dependency outside of poetry.
    config.languages.python.package.pkgs.pjsua2
  ];

  env.LD_LIBRARY_PATH = lib.makeLibraryPath [
    # A native dependency of numpy
    pkgs.zlib
  ];

  languages.python = {
    enable = true;
    poetry.enable = true;
  };
}
