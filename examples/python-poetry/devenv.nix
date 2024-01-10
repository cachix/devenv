{ pkgs, config, ... }:

{
  languages.python.libraries = [
    # A native dependency of numpy
    pkgs.zlib

    # A python dependency outside of poetry.
    config.languages.python.package.pkgs.pjsua2
  ];
  languages.python = {
    enable = true;
    poetry.enable = true;
  };
}
