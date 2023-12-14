{ pkgs, config, lib, ... }:

let
  cfg = config.languages.javascript;

  nodeModulesPath = "node_modules";

  initNpmScript = pkgs.writeShellScript "init-npm.sh" ''
    function _devenv-npm-install()
    {
      # Avoid running "npm install" for every shell.
      # Only run it when the "package-lock.json" file or nodejs version has changed.
      # We do this by storing the nodejs version and a hash of "package-lock.json" in node_modules.
      local ACTUAL_NPM_CHECKSUM="${cfg.package.version}:$(${pkgs.nix}/bin/nix-hash --type sha256 ${cfg.npm.install.directory}/package-lock.json)"
      local NPM_CHECKSUM_FILE="${nodeModulesPath}/package-lock.json.checksum"
      if [ -f "$NPM_CHECKSUM_FILE" ]
        then
          read -r EXPECTED_NPM_CHECKSUM < "$NPM_CHECKSUM_FILE"
        else
          EXPECTED_NPM_CHECKSUM=""
      fi

      if [ "$ACTUAL_NPM_CHECKSUM" != "$EXPECTED_NPM_CHECKSUM" ]
      then
        if ${cfg.package}/bin/npm install ${cfg.npm.install.directory}
        then
          echo "$ACTUAL_NPM_CHECKSUM" > "$NPM_CHECKSUM_FILE"
        else
          echo "Npm install failed. Run 'npm install' manually."
        fi
      fi
    }

    if [ ! -f ${cfg.npm.install.directory}/package.json ]
    then
      echo "No ${cfg.npm.install.directory}/package.json found. Run 'npm init' to create one." >&2
    else
      _devenv-npm-install
    fi
  '';
in
{
  options.languages.javascript = {
    enable = lib.mkEnableOption "tools for JavaScript development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nodejs;
      defaultText = lib.literalExpression "pkgs.nodejs";
      description = "The Node package to use.";
    };

    corepack = {
      enable = lib.mkEnableOption "shims for package managers besides npm";
    };

    npm.install = {
      enable = lib.mkEnableOption "npm install during devenv initialisation";
      directory = lib.mkOption {
        type = lib.types.str;
        description = "";
        default = config.devenv.root;
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.corepack.enable (pkgs.runCommand "corepack-enable" { } ''
      mkdir -p $out/bin
      ${cfg.package}/bin/corepack enable --install-directory $out/bin
    '');

    enterShell = lib.concatStringsSep "\n" (
      (lib.optional cfg.npm.install.enable ''
        source ${initNpmScript}
      '')
    );
  };
}
