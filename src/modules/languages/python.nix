{ pkgs, config, lib, ... }:

let
  cfg = config.languages.python;
in
{
  options.languages.python = {
    enable = lib.mkEnableOption "tools for Python development";

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
        if [ ! -d ${config.env.DEVENV_STATE}/venv ]
        then
          python -m venv ${config.env.DEVENV_STATE}/venv
        fi
        source ${config.env.DEVENV_STATE}/venv/bin/activate
      '') ++ (lib.optional cfg.poetry.enable ''
        if [ ! -f pyproject.toml ]
        then
          echo "No pyproject.toml found. Run 'poetry init' to create one." >&2
        elif [ ! -f poetry.lock ]
        then
          echo "No poetry.lock found. Run 'poetry install' to create one from pyproject.toml." >&2
        else
          # Avoid running "poetry install" for every shell.
          # Only run it when the "poetry.lock" file has changed.
          # We do this by storing a hash of "poetry.lock" in venv.
          ACTUAL_POETRY_CHECKSUM="$(${pkgs.nix}/bin/nix-hash --type sha256 poetry.lock)"
          POETRY_CHECKSUM_FILE="${config.env.DEVENV_STATE}/poetry.lock.checksum"
          if [ -f "$POETRY_CHECKSUM_FILE" ]
          then
            read -r EXPECTED_POETRY_CHECKSUM < "$POETRY_CHECKSUM_FILE"
          else
            EXPECTED_POETRY_CHECKSUM=""
          fi

          if [ "$ACTUAL_POETRY_CHECKSUM" != "$EXPECTED_POETRY_CHECKSUM" ]
          then
            poetry install --no-interaction --quiet
            echo "$ACTUAL_POETRY_CHECKSUM" > "$POETRY_CHECKSUM_FILE"
          fi
        fi
      '')
    );
  };
}
