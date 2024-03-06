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
      local ACTUAL_NPM_CHECKSUM="${cfg.package.version}:$(${pkgs.nix}/bin/nix-hash --type sha256 ${lib.optionalString (cfg.directory != config.devenv.root) ''"${cfg.directory}/"''}package-lock.json)"
      local NPM_CHECKSUM_FILE="${nodeModulesPath}/package-lock.json.checksum"
      if [ -f "$NPM_CHECKSUM_FILE" ]
        then
          read -r EXPECTED_NPM_CHECKSUM < "$NPM_CHECKSUM_FILE"
        else
          EXPECTED_NPM_CHECKSUM=""
      fi

      if [ "$ACTUAL_NPM_CHECKSUM" != "$EXPECTED_NPM_CHECKSUM" ]
      then
        if ${cfg.package}/bin/npm install ${lib.optionalString (cfg.directory != config.devenv.root) "--prefix ${cfg.directory}"}
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
      description = "The Node package to use.";
      example = "pkgs.bun";
    };

    corepack = {
      enable = lib.mkEnableOption "shims for package managers besides npm";
    };

    npm.install = {
      enable = lib.mkEnableOption "npm install during devenv initialisation";
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




