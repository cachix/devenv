{ pkgs, config, lib, ... }:

let
  cfg = config.languages.python;

  initVenvScript = pkgs.writeShellScript "init-venv.sh" ''
    if [ ! -L ${config.env.DEVENV_STATE}/venv/devenv-profile ] \
    || [ "$(${pkgs.coreutils}/bin/readlink ${config.env.DEVENV_STATE}/venv/devenv-profile)" != "${config.env.DEVENV_PROFILE}" ]
    then
      if [ -d ${config.env.DEVENV_STATE}/venv ]
      then
        echo "Rebuilding Python venv..."
        ${pkgs.coreutils}/bin/rm -rf ${config.env.DEVENV_STATE}/venv
      fi
      ${lib.optionalString cfg.poetry.enable ''
        [ -f "${config.env.DEVENV_STATE}/poetry.lock.checksum" ] && rm ${config.env.DEVENV_STATE}/poetry.lock.checksum
      ''}
      python -m venv ${config.env.DEVENV_STATE}/venv
      ln -sf ${config.env.DEVENV_PROFILE} ${config.env.DEVENV_STATE}/venv/devenv-profile
    fi
    source ${config.env.DEVENV_STATE}/venv/bin/activate
  '';

  initPoetryScript = pkgs.writeShellScript "init-poetry.sh" ''
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
        ${cfg.poetry.package}/bin/poetry install --no-interaction --quiet
        echo "$ACTUAL_POETRY_CHECKSUM" > "$POETRY_CHECKSUM_FILE"
      fi
    fi
  '';
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
        default = pkgs.poetry.override {
          python3 = cfg.package;
        };
        defaultText = lib.literalExpression ''
          pkgs.poetry.override {
            python3 = config.languages.python.package;
          }
        '';
        description = "The Poetry package to use.";
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
        source ${initVenvScript}
      '') ++ (lib.optional cfg.poetry.enable ''
        source ${initPoetryScript}
      '')
    );
  };
}
