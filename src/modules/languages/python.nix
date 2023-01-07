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
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.PYTHONPATH = "${config.env.DEVENV_PROFILE}/${cfg.package.sitePackages}";
  };
}
