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

    poetry = {
      enable = lib.mkEnableOption "poetry";
      package = lib.mkOption {
        type = lib.types.package;
        default = cfg.package.pkgs.poetry;
        defaultText = lib.literalExpression "config.languages.python.package.pkgs.poetry";
        description = "The poetry package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    languages.python.venv.enable = lib.mkIf cfg.poetry.enable (lib.mkDefault true);

    packages = [
      cfg.package
    ] ++ (lib.optional cfg.poetry.enable cfg.poetry.package);

    env.PYTHONPATH = "${config.env.DEVENV_PROFILE}/${cfg.package.sitePackages}";

    enterShell = lib.concatStringsSep "\n" (
      (lib.optional cfg.venv.enable ''
        python -m venv ${config.env.DEVENV_STATE}/venv
        source ${config.env.DEVENV_STATE}/venv/bin/activate
      '') ++ (lib.optional cfg.poetry.enable ''
        if [ -f pyproject.toml ]
        then
          poetry install --no-interaction --quiet
        else
          echo "No pyproject.toml found. Run 'poetry init'." >&2
        fi
      '')
    );
  };
}
