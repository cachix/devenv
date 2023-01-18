{ pkgs, config, lib, ... }:

let
  cfg = config.languages.python;
in
{
  options.languages.python = {
    enable = lib.mkEnableOption "Enable tools for Python development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.python3;
      defaultText = lib.literalExpression "pkgs.python3";
      description = "The Python package to use.";
    };

    venv.enable = lib.mkEnableOption "Python virtual environment";
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.PYTHONPATH = "${config.env.DEVENV_PROFILE}/${cfg.package.sitePackages}";

    enterShell = lib.mkIf cfg.venv.enable ''
      if [ ! -d ${config.env.DEVENV_STATE}/venv ]
      then
        python -m venv ${config.env.DEVENV_STATE}/venv
      fi
      source ${config.env.DEVENV_STATE}/venv/bin/activate
    '';
  };
}
