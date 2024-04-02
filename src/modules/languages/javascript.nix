{ pkgs, config, lib, ... }:

let
  cfg = config.languages.javascript;

  nodeModulesPath = "${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}node_modules";

  initNpmScript = pkgs.writeShellScript "init-npm.sh" ''
    function _devenv-npm-install()
    {
      # Avoid running "npm install" for every shell.
      # Only run it when the "package-lock.json" file or nodejs version has changed.
      # We do this by storing the nodejs version and a hash of "package-lock.json" in node_modules.
      local ACTUAL_NPM_CHECKSUM="${cfg.npm.package.version}:$(${pkgs.nix}/bin/nix-hash --type sha256 ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}package-lock.json)"
      local NPM_CHECKSUM_FILE="${nodeModulesPath}/package-lock.json.checksum"
      if [ -f "$NPM_CHECKSUM_FILE" ]
        then
          read -r EXPECTED_NPM_CHECKSUM < "$NPM_CHECKSUM_FILE"
        else
          EXPECTED_NPM_CHECKSUM=""
      fi

      if [ "$ACTUAL_NPM_CHECKSUM" != "$EXPECTED_NPM_CHECKSUM" ]
      then
        if ${cfg.npm.package}/bin/npm install ${lib.optionalString (cfg.directory != config.devenv.root) "--prefix ${cfg.directory}"}
        then
          echo "$ACTUAL_NPM_CHECKSUM" > "$NPM_CHECKSUM_FILE"
        else
          echo "Install failed. Run 'npm install' manually."
        fi
      fi
    }

    if [ ! -f ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}package.json ]
    then
      echo "No package.json found${lib.optionalString (cfg.directory != config.devenv.root) ''"in ${cfg.directory}"''}. Run '${lib.optionalString (cfg.directory != config.devenv.root) ''"cd ${cfg.directory}/ && "''}npm init' to create one." >&2
    else
      _devenv-npm-install
    fi
  '';

  initPnpmScript = pkgs.writeShellScript "init-pnpm.sh" ''
    function _devenv-pnpm-install()
    {
      # Avoid running "pnpm install" for every shell.
      # Only run it when the "package-lock.json" file or nodejs version has changed.
      # We do this by storing the nodejs version and a hash of "package-lock.json" in node_modules.
      local ACTUAL_PNPM_CHECKSUM="${cfg.pnpm.package.version}:$(${pkgs.nix}/bin/nix-hash --type sha256 ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}pnpm-lock.yaml)"
      local PNPM_CHECKSUM_FILE="${nodeModulesPath}/pnpm-lock.yaml.checksum"
      if [ -f "$PNPM_CHECKSUM_FILE" ]
        then
          read -r EXPECTED_PNPM_CHECKSUM < "$PNPM_CHECKSUM_FILE"
        else
          EXPECTED_PNPM_CHECKSUM=""
      fi

      if [ "$ACTUAL_PNPM_CHECKSUM" != "$EXPECTED_PNPM_CHECKSUM" ]
      then
        if ${cfg.pnpm.package}/bin/pnpm install ${lib.optionalString (cfg.directory != config.devenv.root) "--dir ${cfg.directory}"}
        then
          echo "$ACTUAL_PNPM_CHECKSUM" > "$PNPM_CHECKSUM_FILE"
        else
          echo "Install failed. Run 'pnpm install' manually."
        fi
      fi
    }

    if [ ! -f ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}package.json ]
    then
      echo "No package.json found${lib.optionalString (cfg.directory != config.devenv.root) ''"in ${cfg.directory}"''}. Run '${lib.optionalString (cfg.directory != config.devenv.root) ''"cd ${cfg.directory}/ && "''}pnpm init' to create one." >&2
    else
      _devenv-pnpm-install
    fi
  '';

  initYarnScript = pkgs.writeShellScript "init-yarn.sh" ''
    function _devenv-yarn-install()
    {
      # Avoid running "yarn install" for every shell.
      # Only run it when the "yarn.lock" file or nodejs version has changed.
      # We do this by storing the nodejs version and a hash of "yarn.lock" in node_modules.
      local ACTUAL_YARN_CHECKSUM="${cfg.yarn.package.version}:$(${pkgs.nix}/bin/nix-hash --type sha256 ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}yarn.lock)"
      local YARN_CHECKSUM_FILE="${nodeModulesPath}/yarn.lock.checksum"
      if [ -f "$YARN_CHECKSUM_FILE" ]
        then
          read -r EXPECTED_YARN_CHECKSUM < "$YARN_CHECKSUM_FILE"
        else
          EXPECTED_YARN_CHECKSUM=""
      fi

      if [ "$ACTUAL_YARN_CHECKSUM" != "$EXPECTED_YARN_CHECKSUM" ]
      then
        if ${cfg.yarn.package}/bin/yarn install ${lib.optionalString (cfg.directory != config.devenv.root) "--cwd ${cfg.directory}"}
        then
          echo "$ACTUAL_YARN_CHECKSUM" > "$YARN_CHECKSUM_FILE"
        else
          echo "Install failed. Run 'yarn install' manually."
        fi
      fi
    }

    if [ ! -f ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}package.json ]
    then
      echo "No package.json found${lib.optionalString (cfg.directory != config.devenv.root) ''"in ${cfg.directory}"''}. Run '${lib.optionalString (cfg.directory != config.devenv.root) ''"cd ${cfg.directory}/ && "''}yarn init' to create one." >&2
    else
      _devenv-yarn-install
    fi
  '';

  initBunScript = pkgs.writeShellScript "init-bun.sh" ''
    function _devenv-bun-install()
    {
      # Avoid running "bun install --yarn" for every shell.
      # Only run it when the "yarn.lock" file or nodejs version has changed.
      # We do this by storing the nodejs version and a hash of "yarn.lock" in node_modules.
      local ACTUAL_BUN_CHECKSUM="${cfg.bun.package.version}:$(${pkgs.nix}/bin/nix-hash --type sha256 ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}yarn.lock)"
      local BUN_CHECKSUM_FILE="${nodeModulesPath}/yarn.lock.checksum"
      if [ -f "$BUN_CHECKSUM_FILE" ]
        then
          read -r EXPECTED_BUN_CHECKSUM < "$BUN_CHECKSUM_FILE"
        else
          EXPECTED_BUN_CHECKSUM=""
      fi

      if [ "$ACTUAL_BUN_CHECKSUM" != "$EXPECTED_BUN_CHECKSUM" ]
      then
        if ${cfg.bun.package}/bin/bun install --yarn ${lib.optionalString (cfg.directory != config.devenv.root) "--cwd ${cfg.directory}"}
        then
          echo "$ACTUAL_BUN_CHECKSUM" > "$BUN_CHECKSUM_FILE"
        else
          echo "Install failed. Run 'bun install --yarn' manually."
        fi
      fi
    }

    if [ ! -f ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}package.json ]
    then
      echo "No package.json found${lib.optionalString (cfg.directory != config.devenv.root) ''"in ${cfg.directory}"''}. Run '${lib.optionalString (cfg.directory != config.devenv.root) ''"cd ${cfg.directory}/ && "''}bun init' to create one." >&2
    else
      _devenv-bun-install
    fi
  '';
in
{
  options.languages.javascript = {
    enable = lib.mkEnableOption "tools for JavaScript development";

    directory = lib.mkOption {
      type = lib.types.str;
      default = config.devenv.root;
      defaultText = lib.literalExpression "config.devenv.root";
      description = ''
        The JavaScript project's root directory. Defaults to the root of the devenv project.
        Can be an absolute path or one relative to the root of the devenv project.
      '';
      example = "./directory";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nodejs-slim;
      defaultText = lib.literalExpression "pkgs.nodejs-slim";
      description = "The Node.js package to use.";
    };

    corepack = {
      enable = lib.mkEnableOption "wrappers for npm, pnpm and Yarn via Node.js Corepack";
    };

    npm = {
      enable = lib.mkEnableOption "install npm";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.nodejs;
        defaultText = lib.literalExpression "pkgs.nodejs";
        description = "The Node.js package to use.";
      };
      install.enable = lib.mkEnableOption "npm install during devenv initialisation";
    };

    pnpm = {
      enable = lib.mkEnableOption "install pnpm";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.nodePackages.pnpm;
        defaultText = lib.literalExpression "pkgs.nodePackages.pnpm";
        description = "The pnpm package to use.";
      };
      install.enable = lib.mkEnableOption "pnpm install during devenv initialisation";
    };

    yarn = {
      enable = lib.mkEnableOption "install yarn";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.yarn;
        defaultText = lib.literalExpression "pkgs.yarn";
        description = "The yarn package to use.";
      };
      install.enable = lib.mkEnableOption "yarn install during devenv initialisation";
    };

    bun = {
      enable = lib.mkEnableOption "install bun";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.bun;
        defaultText = lib.literalExpression "pkgs.bun";
        description = "The bun package to use.";
      };
      install.enable = lib.mkEnableOption "bun install during devenv initialisation";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ]
    ++ lib.optional cfg.npm.enable (cfg.npm.package)
    ++ lib.optional cfg.pnpm.enable (cfg.pnpm.package)
    ++ lib.optional cfg.yarn.enable (cfg.yarn.package)
    ++ lib.optional cfg.bun.enable (cfg.bun.package)
    ++ lib.optional cfg.corepack.enable (pkgs.runCommand "corepack-enable" { } ''
      mkdir -p $out/bin
      ${cfg.package}/bin/corepack enable --install-directory $out/bin
    '');

    enterShell = lib.concatStringsSep "\n" (
      (lib.optional cfg.npm.install.enable ''
        source ${initNpmScript}
      '') ++
      (lib.optional cfg.pnpm.install.enable ''
        source ${initPnpmScript}
      '') ++
      (lib.optional cfg.yarn.install.enable ''
        source ${initYarnScript}
      '') ++
      (lib.optional cfg.bun.install.enable ''
        source ${initBunScript}
      '')
    );
  };
}




