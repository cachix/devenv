{ pkgs, config, lib, ... }:

let
  cfg = config.dotenv;

  dotenvPath = config.devenv.root + "/" + cfg.filename;
in
{
  options.dotenv = {
    enable = lib.mkEnableOption ".env integration, doesn't support comments or multiline values.";

    filename = lib.mkOption {
      type = lib.types.str;
      default = ".env";
      description = ''
        The name of the dotenv file to load.
      '';
    };

    disableHint = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Disable the hint that are printed when the dotenv module is not enabled, but .env is present.
      '';
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      enterShell = ''
        if test -f ${lib.escapeShellArg dotenvPath}
        then
          export $(xargs < ${lib.escapeShellArg dotenvPath})
        fi
      '';
    })
    (lib.mkIf cfg.enable (
      {
        enterShell = ''
          # Test if the file exists, and if not, print a hint.
          if ! test -f ${lib.escapeShellArg dotenvPath}
          then
            echo "💡 A ${cfg.filename} file was not found, while dotenv integration is enabled."
            echo 
            if test -f ${lib.escapeShellArg "${dotenvPath}.example}"}
            then
              echo "   To create .env, you can copy the example file:"
              echo
              echo "   $ cp ${dotenvPath}.example ${dotenvPath}";
              echo
            fi
            echo "   To disable it, add \`dotenv.enable = false;\` to your devenv.nix file.";
            echo
            echo "See https://devenv.sh/integrations/dotenv/ for more information.";
          fi
        '';
      }
    ))
    (lib.mkIf (!cfg.enable && !cfg.disableHint) {
      enterShell = ''
        if test -f ${lib.escapeShellArg dotenvPath}
        then
          echo "💡 A ${cfg.filename} file found, while dotenv integration is currently not enabled."
          echo 
          echo "   To enable it, add \`dotenv.enable = true;\` to your devenv.nix file.";
          echo "   To disable this hint, add \`dotenv.disableHint = true;\` to your devenv.nix file.";
          echo
          echo "See https://devenv.sh/integrations/dotenv/ for more information.";
        fi
      '';
    })
  ];
}
