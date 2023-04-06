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
    function _devenv-init-poetry-venv()
    {
      if [ ! -d ${config.env.DEVENV_ROOT}/.venv ] \
        || [ ! "$(readlink ${config.env.DEVENV_ROOT}/.venv/bin/python)" -ef "${cfg.package.interpreter}" ]
      then
        poetry env use --no-interaction ${cfg.package.interpreter}
      fi
    }

    function _devenv-poetry-install()
    {
      # Avoid running "poetry install" for every shell.
      # Only run it when the "poetry.lock" file or python interpreter has changed.
      # We do this by storing the interpreter path and a hash of "poetry.lock" in venv.
      local ACTUAL_POETRY_CHECKSUM="${cfg.package.interpreter}:$(${pkgs.nix}/bin/nix-hash --type sha256 poetry.lock)"
      local POETRY_CHECKSUM_FILE="${config.env.DEVENV_ROOT}/.venv/poetry.lock.checksum"
      if [ -f "$POETRY_CHECKSUM_FILE" ]
      then
        read -r EXPECTED_POETRY_CHECKSUM < "$POETRY_CHECKSUM_FILE"
      else
        EXPECTED_POETRY_CHECKSUM=""
      fi

      if [ "$ACTUAL_POETRY_CHECKSUM" != "$EXPECTED_POETRY_CHECKSUM" ]
      then
        if ${cfg.poetry.package}/bin/poetry install --no-interaction ${lib.concatStringsSep " " cfg.poetry.install.arguments}
        then
          echo "$ACTUAL_POETRY_CHECKSUM" > "$POETRY_CHECKSUM_FILE"
        else
          echo "Poetry install failed. Run 'poetry install' manually."
        fi
      fi
    }

    if [ ! -f pyproject.toml ]
    then
      echo "No pyproject.toml found. Run 'poetry init' to create one." >&2
    else
      _devenv-init-poetry-venv
      ${lib.optionalString cfg.poetry.install.enable ''
        _devenv-poetry-install
      ''}
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
      install = {
        enable = lib.mkEnableOption "poetry install during devenv initialisation";
        arguments = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          description = "Command line arguments pass to `poetry install` during devenv initialisation.";
          internal = true;
        };
        installRootPackage = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether the root package (your project) should be installed. See `--no-root`";
        };
        quiet = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether `poetry install` should avoid outputting messages during devenv initialisation.";
        };
      };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.poetry;
        defaultText = lib.literalExpression "pkgs.poetry";
        description = "The Poetry package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    languages.python.poetry.install.enable = lib.mkIf cfg.poetry.enable (lib.mkDefault true);
    languages.python.poetry.install.arguments =
      lib.optional (!cfg.poetry.install.installRootPackage) "--no-root" ++
      lib.optional cfg.poetry.install.quiet "--quiet";

    packages = [
      cfg.package
    ] ++ (lib.optional cfg.poetry.enable cfg.poetry.package);

    env = {
      PYTHONPATH = "${config.env.DEVENV_PROFILE}/${cfg.package.sitePackages}";
    } // (lib.optionalAttrs cfg.poetry.enable {
      # Make poetry use DEVENV_ROOT/.venv
      POETRY_VIRTUALENVS_IN_PROJECT = "true";
      # Make poetry create the local virtualenv when it does not exist.
      POETRY_VIRTUALENVS_CREATE = "true";
      # Make poetry stop accessing any other virtualenvs in $HOME.
      POETRY_VIRTUALENVS_PATH = "/var/empty";
    });

    enterShell = lib.concatStringsSep "\n" (
      (lib.optional cfg.venv.enable ''
        source ${initVenvScript}
      '') ++ (lib.optional cfg.poetry.install.enable ''
        source ${initPoetryScript}
      '')
    );
  };
}
