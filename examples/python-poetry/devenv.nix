{ pkgs, config, ... }:

{
  packages = [
    # A python dependency outside of poetry.
    config.languages.python.package.pkgs.pjsua2
  ];

  languages.python = {
    enable = true;
    libraries = [ pkgs.zlib ];
    poetry.enable = true;
  };
}
