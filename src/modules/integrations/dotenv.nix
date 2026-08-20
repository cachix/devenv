{ config
, lib
, options
, ...
}:

let
  cfg = config.dotenv;

  normalizeFilenames = filenames: if lib.isList filenames then filenames else [ filenames ];
  cliOwnedNames = [ "SHELL" "DEVENV_CMDLINE" ];
  dotenvEnvModuleLocation = "devenv:dotenv-resolved";
  dotenvEnvModule =
    { config
    , devenvPrimops ? { }
    , lib
    , ...
    }:
    let
      loadDotenv = devenvPrimops.loadDotenv or (
        _filenames: _substitution:
          throw ''
            The dotenv integration requires the C-Nix devenv CLI. It is not
            available through the flake integration or another standalone Nix evaluation.
          ''
      );
    in
    {
      _file = dotenvEnvModuleLocation;
      config = lib.mkIf config.dotenv.enable {
        dotenv.resolved = builtins.removeAttrs
          (loadDotenv (normalizeFilenames config.dotenv.filename) config.dotenv.substitution)
          cliOwnedNames;
        env = lib.mapAttrs (_name: value: lib.mkDefault value) config.dotenv.resolved;
      };
    };
in
{
  imports = [ dotenvEnvModule ];

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

  config = {
    # Runtime loading happens again after enter-shell tasks. Definitions from
    # outside the dotenv injection module remain Nix-owned even when their
    # values happen to equal the initial dotenv value.
    dotenv.reservedNames = lib.unique (
      lib.concatMap
        (definition:
          lib.optionals (definition.file != dotenvEnvModuleLocation) (
            builtins.attrNames definition.value
          )
        )
        options.env.definitionsWithLocations
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
  };
}
