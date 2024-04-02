{ pkgs, config, lib, ... }:

let
  cfg = config.languages.python;
  libraries = lib.makeLibraryPath (
    cfg.libraries
    ++ (lib.optional cfg.manylinux.enable pkgs.pythonManylinuxPackages.manylinux2014Package)
    # see https://matrix.to/#/!kjdutkOsheZdjqYmqp:nixos.org/$XJ5CO4bKMevYzZq_rrNo64YycknVFJIJTy6hVCJjRlA?via=nixos.org&via=matrix.org&via=nixos.dev
    ++ [ pkgs.stdenv.cc.cc.lib ]
  );

  readlink = "${pkgs.coreutils}/bin/readlink -f ";
  package = pkgs.callPackage "${pkgs.path}/pkgs/development/interpreters/python/wrapper.nix" {
    python = cfg.package;
    requiredPythonModules = cfg.package.pkgs.requiredPythonModules;
    makeWrapperArgs = [
      "--prefix"
      "LD_LIBRARY_PATH"
      ":"
      libraries
    ] ++ lib.optionals pkgs.stdenv.isDarwin [
      "--prefix"
      "DYLD_LIBRARY_PATH"
      ":"
      libraries
    ];
  };

  requirements = pkgs.writeText "requirements.txt" (
    if lib.isPath cfg.venv.requirements
    then builtins.readFile cfg.venv.requirements
    else cfg.venv.requirements
  );

  nixpkgs-python = config.lib.getInput {
    name = "nixpkgs-python";
    url = "github:cachix/nixpkgs-python";
    attribute = "languages.python.version";
    follows = [ "nixpkgs" ];
  };

  initVenvScript = pkgs.writeShellScript "init-venv.sh" ''
    # Make sure any tools are not attempting to use the Python interpreter from any
    # existing virtual environment. For instance if devenv was started within an venv.
    unset VIRTUAL_ENV

    VENV_PATH="${config.env.DEVENV_STATE}/${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}venv"

    profile_python="$(${readlink} ${package.interpreter})"
    devenv_interpreter_path="$(${pkgs.coreutils}/bin/cat "$VENV_PATH/.devenv_interpreter" 2> /dev/null || echo false )"
    venv_python="$(${readlink} "$devenv_interpreter_path")"
    requirements="${lib.optionalString (cfg.venv.requirements != null) ''${requirements}''}"

    # recreate venv if necessary
    if [ -z $venv_python ] || [ $profile_python != $venv_python ]
    then
      echo "Python interpreter changed, rebuilding Python venv..."
      ${pkgs.coreutils}/bin/rm -rf "$VENV_PATH"
      ${lib.optionalString cfg.poetry.enable ''
        [ -f "${config.env.DEVENV_STATE}/poetry.lock.checksum" ] && rm ${config.env.DEVENV_STATE}/poetry.lock.checksum
      ''}
      echo ${package.interpreter} -m venv --upgrade-deps "$VENV_PATH"
      ${package.interpreter} -m venv --upgrade-deps "$VENV_PATH"
      echo "${package.interpreter}" > "$VENV_PATH/.devenv_interpreter"
    fi

    source "$VENV_PATH"/bin/activate

    # reinstall requirements if necessary
    if [ -n "$requirements" ]
      then
        devenv_requirements_path="$(${pkgs.coreutils}/bin/cat "$VENV_PATH/.devenv_requirements" 2> /dev/null|| echo false )"
        devenv_requirements="$(${readlink} "$devenv_requirements_path")"
        if [ -z $devenv_requirements ] || [ $devenv_requirements != $requirements ]
          then
            echo "${requirements}" > "$VENV_PATH/.devenv_requirements"
            echo "Requirements changed, running pip install -r ${requirements}..."
           "$VENV_PATH"/bin/pip install -r ${requirements}
       fi
    fi
  '';

  initPoetryScript = pkgs.writeShellScript "init-poetry.sh" ''
    function _devenv_init_poetry_venv
    {
      # Make sure any tools are not attempting to use the Python interpreter from any
      # existing virtual environment. For instance if devenv was started within an venv.
      unset VIRTUAL_ENV

      # Make sure poetry's venv uses the configured Python executable.
      ${cfg.poetry.package}/bin/poetry env use --no-interaction --quiet ${package.interpreter}
    }

    function _devenv_poetry_install
    {
      local POETRY_INSTALL_COMMAND=(${cfg.poetry.package}/bin/poetry install --no-interaction ${lib.concatStringsSep " " cfg.poetry.install.arguments} ${lib.optionalString (cfg.directory != config.devenv.root) ''--directory=${cfg.directory}''})
      # Avoid running "poetry install" for every shell.
      # Only run it when the "poetry.lock" file or Python interpreter has changed.
      # We do this by storing the interpreter path and a hash of "poetry.lock" in venv.
      local ACTUAL_POETRY_CHECKSUM="${package.interpreter}:$(${pkgs.nix}/bin/nix-hash --type sha256 "$DEVENV_ROOT"/${lib.optionalString (cfg.directory != config.devenv.root) ''${cfg.directory}/''}pyproject.toml):$(${pkgs.nix}/bin/nix-hash --type sha256 "$DEVENV_ROOT"/${lib.optionalString (cfg.directory != config.devenv.root) ''${cfg.directory}/''}poetry.lock):''${POETRY_INSTALL_COMMAND[@]}"
      local POETRY_CHECKSUM_FILE="$DEVENV_ROOT"/${lib.optionalString (cfg.directory != config.devenv.root) ''${cfg.directory}/''}.venv/poetry.lock.checksum
      if [ -f "$POETRY_CHECKSUM_FILE" ]
      then
        read -r EXPECTED_POETRY_CHECKSUM < "$POETRY_CHECKSUM_FILE"
      else
        EXPECTED_POETRY_CHECKSUM=""
      fi

      if [ "$ACTUAL_POETRY_CHECKSUM" != "$EXPECTED_POETRY_CHECKSUM" ]
      then
        if ''${POETRY_INSTALL_COMMAND[@]}
        then
          echo "$ACTUAL_POETRY_CHECKSUM" > "$POETRY_CHECKSUM_FILE"
        else
          echo "Poetry install failed. Run 'poetry install' manually."
        fi
      fi
    }

    if [ ! -f "$DEVENV_ROOT"/${lib.optionalString (cfg.directory != config.devenv.root) ''${cfg.directory}/''}pyproject.toml ]
    then
      echo "No pyproject.toml found. Run 'poetry init' to create one." >&2
    else
      _devenv_init_poetry_venv
      ${lib.optionalString cfg.poetry.install.enable ''
        _devenv_poetry_install
      ''}
      ${lib.optionalString cfg.poetry.activate.enable ''
        source "$DEVENV_ROOT"/${lib.optionalString (cfg.directory != config.devenv.root) ''${cfg.directory}/''}.venv/bin/activate
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

    manylinux.enable = lib.mkOption {
      type = lib.types.bool;
      default = pkgs.stdenv.isLinux;
      description = ''
        Whether to install manylinux2014 libraries.

        Enabled by default on linux;

        This is useful when you want to use Python wheels that depend on manylinux2014 libraries.
      '';
    };

    libraries = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ "${config.devenv.dotfile}/profile" ];
      description = ''
        Additional libraries to make available to the Python interpreter.

        This is useful when you want to use Python wheels that depend on native libraries.
      '';
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Python version to use.
        This automatically sets the `languages.python.package` using [nixpkgs-python](https://github.com/cachix/nixpkgs-python).
      '';
      example = "3.11 or 3.11.2";
    };

    directory = lib.mkOption {
      type = lib.types.str;
      default = config.devenv.root;
      defaultText = lib.literalExpression "config.devenv.root";
      description = ''
        The Python project's root directory. Defaults to the root of the devenv project.
        Can be an absolute path or one relative to the root of the devenv project.
      '';
      example = "./directory";
    };

    venv.enable = lib.mkEnableOption "Python virtual environment";

    venv.requirements = lib.mkOption {
      type = lib.types.nullOr (lib.types.either lib.types.lines lib.types.path);
      default = null;
      description = ''
        Contents of pip requirements.txt file.
        This is passed to `pip install -r` during `devenv shell` initialisation.
      '';
    };

    venv.quiet = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Whether `pip install` should avoid outputting messages during devenv initialisation.";
    };

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
        onlyInstallRootPackage = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether to only install the root package (your project) should be installed, but no dependencies. See `--only-root`";
        };
        compile = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether `poetry install` should compile Python source files to bytecode.";
        };
        quiet = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether `poetry install` should avoid outputting messages during devenv initialisation.";
        };
        groups = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          description = "Which dependency groups to install. See `--with`.";
        };
        ignoredGroups = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          description = "Which dependency groups to ignore. See `--without`.";
        };
        onlyGroups = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          description = "Which dependency groups to exclusively install. See `--only`.";
        };
        extras = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          description = "Which extras to install. See `--extras`.";
        };
        allExtras = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether to install all extras. See `--all-extras`.";
        };
        verbosity = lib.mkOption {
          type = lib.types.enum [ "no" "little" "more" "debug" ];
          default = "no";
          description = "What level of verbosity the output of `poetry install` should have.";
        };
      };
      activate.enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Whether to activate the poetry virtual environment automatically.";
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
      lib.optional cfg.poetry.install.onlyInstallRootPackage "--only-root" ++
      lib.optional (!cfg.poetry.install.installRootPackage && !cfg.poetry.install.onlyInstallRootPackage) "--no-root" ++
      lib.optional cfg.poetry.install.compile "--compile" ++
      lib.optional cfg.poetry.install.quiet "--quiet" ++
      lib.optionals (cfg.poetry.install.groups != [ ]) [ "--with" ''"${lib.concatStringsSep "," cfg.poetry.install.groups}"'' ] ++
      lib.optionals (cfg.poetry.install.ignoredGroups != [ ]) [ "--without" ''"${lib.concatStringsSep "," cfg.poetry.install.ignoredGroups}"'' ] ++
      lib.optionals (cfg.poetry.install.onlyGroups != [ ]) [ "--only" ''"${lib.concatStringsSep " " cfg.poetry.install.onlyGroups}"'' ] ++
      lib.optionals (cfg.poetry.install.extras != [ ]) [ "--extras" ''"${lib.concatStringsSep " " cfg.poetry.install.extras}"'' ] ++
      lib.optional cfg.poetry.install.allExtras "--all-extras" ++
      lib.optional (cfg.poetry.install.verbosity == "little") "-v" ++
      lib.optional (cfg.poetry.install.verbosity == "more") "-vv" ++
      lib.optional (cfg.poetry.install.verbosity == "debug") "-vvv";

    languages.python.poetry.activate.enable = lib.mkIf cfg.poetry.enable (lib.mkDefault true);

    languages.python.package = lib.mkMerge [
      (lib.mkIf (cfg.version != null)
        (nixpkgs-python.packages.${pkgs.stdenv.system}.${cfg.version} or (throw "Unsupported Python version, see https://github.com/cachix/nixpkgs-python#supported-python-versions")))
    ];

    cachix.pull = lib.mkIf (cfg.version != null) [ "nixpkgs-python" ];

    packages = [
      package
    ] ++ (lib.optional cfg.poetry.enable cfg.poetry.package);

    env = lib.optionalAttrs cfg.poetry.enable {
      # Make poetry use DEVENV_ROOT/.venv
      POETRY_VIRTUALENVS_IN_PROJECT = "true";
      # Make poetry create the local virtualenv when it does not exist.
      POETRY_VIRTUALENVS_CREATE = "true";
      # Make poetry stop accessing any other virtualenvs in $HOME.
      POETRY_VIRTUALENVS_PATH = "/var/empty";
    };

    enterShell = lib.concatStringsSep "\n" ([
      ''
        export PYTHONPATH="$DEVENV_PROFILE/${package.sitePackages}''${PYTHONPATH:+:$PYTHONPATH}"
      ''
    ] ++
    (lib.optional cfg.venv.enable ''
      source ${initVenvScript}
    '') ++ (lib.optional cfg.poetry.install.enable ''
      source ${initPoetryScript}
    '')
    );
  };
}
