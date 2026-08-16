{ config, lib, ... }:

let
  cfg = config.dotenv;

  normalizeFilenames = filenames: if lib.isList filenames then filenames else [ filenames ];
  cliOwnedNames = [ "SHELL" "DEVENV_CMDLINE" ];
  devenvPrimops = config._module.args.devenvPrimops or { };
  loadDotenv = devenvPrimops.loadDotenv or (
    _filenames: _substitution:
      throw ''
        The dotenv integration requires the C-Nix devenv CLI. It is not
        available through the flake integration or another standalone Nix evaluation.
      ''
  );
in
{
  options.dotenv = {
    enable = lib.mkEnableOption ".env integration";

    filename = lib.mkOption {
      type = lib.types.either lib.types.str (lib.types.listOf lib.types.str);
      default = ".env";
      description = "The path of the dotenv file to load, or a list of dotenv files to load in order of precedence.";
    };

    substitution = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to expand variable references such as `$NAME`, `''${NAME}`, and
        `''${NAME:-default}` in dotenv values. Disabled by default so dollar signs
        in passwords, hashes, and tokens remain literal.
      '';
    };

    resolved = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      internal = true;
      description = "Dotenv values returned by the devenv CLI primop.";
    };

    reservedNames = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      internal = true;
      description = "Environment variable names owned by the Nix configuration.";
    };

    disableHint = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Disable the hint printed when a dotenv file is present but the integration is not enabled.";
    };
  };

  config = lib.mkMerge [
    {
      # Runtime loading happens again after enter-shell tasks. Reserve only
      # values that differ from the dotenv defaults so newly generated dotenv
      # files are not filtered merely because a later eval has discovered them.
      dotenv.reservedNames = builtins.attrNames (
        lib.filterAttrs
          (
            name: value:
              !builtins.hasAttr name cfg.resolved
              || !builtins.isString value
              || value != cfg.resolved.${name}
          )
          config.env
      );

      assertions = [
        {
          assertion = !(cfg.enable && config.devenv.flakesIntegration);
          message = ''
            The dotenv integration is loaded by the devenv CLI and is not
            supported by the flake integration. Use `devenv shell`, or load the
            file separately in your flake-based shell.
          '';
        }
      ];
    }

    (lib.mkIf cfg.enable {
      dotenv.resolved = builtins.removeAttrs
        (loadDotenv (normalizeFilenames cfg.filename) cfg.substitution)
        cliOwnedNames;
      env = lib.mapAttrs (_name: value: lib.mkDefault value) cfg.resolved;
    })
  ];
}
