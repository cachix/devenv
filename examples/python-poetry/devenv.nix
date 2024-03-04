{ pkgs, config, ... }:

{
  packages = [
    # A python dependency outside of poetry.
    config.languages.python.package.pkgs.pjsua2
  ];

  # this envvar can be removed and the lib can be moved into
  # languages.python.libraries when we start working against env-venv
  env.LD_LIBRARY_PATH = lib.makeLibraryPath [
    # A native dependency of numpy
    pkgs.zlib
  ];

  languages.python = {
    enable = true;
    poetry = {
      enable = true;
      install = {
        enable = true;
        installRootPackage = false;
        onlyInstallRootPackage = false;
        compile = false;
        quiet = false;
        groups = [ ];
        ignoredGroups = [ ];
        onlyGroups = [ ];
        extras = [ ];
        allExtras = false;
        verbosity = "no";
      };
      activate.enable = true;
      package = pkgs.poetry;
    };
  };
}
