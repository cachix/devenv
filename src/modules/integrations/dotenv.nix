{ pkgs, config, lib, self, ... }:

let
  cfg = config.dotenv;

  normalizeFilenames = filenames: if lib.isList filenames then filenames else [ filenames ];
  dotenvFiles = normalizeFilenames cfg.filename;
  dotenvPaths = map (filename: (self + ("/" + filename))) dotenvFiles;

  parseLine = line:
    let
      parts = builtins.match "([^[:space:]=#]+)[[:space:]]*=[[:space:]]*(.*)" line;
    in
    if (!builtins.isNull parts) && (builtins.length parts) == 2 then
      { name = builtins.elemAt parts 0; value = builtins.elemAt parts 1; }
    else
      null;

  parseEnvFile = content: builtins.listToAttrs (lib.filter (x: !builtins.isNull x) (map parseLine (lib.splitString "\n" content)));

  mergeEnvFiles = files: lib.foldl' (acc: file: lib.recursiveUpdate acc (if lib.pathExists file then parseEnvFile (builtins.readFile file) else { })) { } files;

  createMissingFileMessage = file:
    let
      exampleExists = lib.pathExists (file + ".example");
      filename = builtins.baseNameOf (toString file);
    in
    lib.optionalString (!lib.pathExists file) ''
      echo "💡 The dotenv file '${filename}' was not found."
      ${lib.optionalString exampleExists ''
        echo
        echo "   To create this file, you can copy the example file:"
        echo
        echo "   $ cp ${filename}.example ${filename}"
        echo
      ''}
    '';
in
{
  options.dotenv = {
    enable = lib.mkEnableOption ".env integration, doesn't support comments or multiline values.";

    filename = lib.mkOption {
      type = lib.types.either lib.types.str (lib.types.listOf lib.types.str);
      default = ".env";
      description = "The name of the dotenv file to load, or a list of dotenv files to load in order of precedence.";
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
      enterShell = lib.concatStringsSep "\n" (map createMissingFileMessage dotenvPaths);
      dotenv.resolved = mergeEnvFiles dotenvPaths;
      assertions = [{
        assertion = builtins.all (lib.hasPrefix ".env") dotenvFiles;
        message = "The dotenv filename must start with '.env'.";
      }];
    })
    (lib.mkIf (!cfg.enable && !cfg.disableHint) {
      enterShell =
        let
          dotenvFound = lib.any lib.pathExists dotenvPaths;
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
