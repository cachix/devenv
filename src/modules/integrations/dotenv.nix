{ pkgs, config, lib, ... }:

let
  cfg = config.dotenv;

  dotenvFiles = if lib.length cfg.filenames > 0 then cfg.filenames else [ cfg.filename ];
  dotenvPaths = map (filename: config.devenv.root + "/" + filename) dotenvFiles;

  parseLine = line:
    let
      parts = builtins.match "(.+) *= *(.+)" line;
    in
    if (!builtins.isNull parts) && (builtins.length parts) == 2 then
      { name = builtins.elemAt parts 0; value = builtins.elemAt parts 1; }
    else
      null;

  parseEnvFile = content: builtins.listToAttrs (lib.filter (x: !builtins.isNull x) (map parseLine (lib.splitString "\n" content)));

  mergeEnvFiles = files: lib.foldl' (acc: file: lib.recursiveUpdate acc (if lib.pathExists file then parseEnvFile (builtins.readFile file) else { })) { } files;
in
{
  options.dotenv = {
    enable = lib.mkEnableOption ".env integration, doesn't support comments or multiline values.";

    filename = lib.mkOption {
      type = lib.types.str;
      default = ".env";
      description = "The name of the primary dotenv file to load.";
    };

    filenames = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "The list of dotenv files to load, in order of precedence. Overrides the `filename` option if provided.";
    };

    resolved = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      internal = true;
    };

    disableHint = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Disable the hint that are printed when the dotenv module is not enabled, but .env is present.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      env = lib.mapAttrs (name: value: lib.mkDefault value) config.dotenv.resolved;
      dotenv.resolved = mergeEnvFiles dotenvPaths;
    })
    (lib.mkIf (!cfg.enable && !cfg.disableHint) {
      enterShell =
        let
          dotenvFound = lib.any (file: lib.pathExists file) dotenvPaths;
        in
        lib.optionalString dotenvFound ''
          echo "💡 A dotenv file was found, while dotenv integration is currently not enabled."
          echo 
          echo "   To enable it, add \`dotenv.enable = true;\` to your devenv.nix file.";
          echo "   To disable this hint, add \`dotenv.disableHint = true;\` to your devenv.nix file.";
          echo
          echo "See https://devenv.sh/integrations/dotenv/ for more information.";
        '';
    })
  ];
}
