{ pkgs, lib, config, ... }:
let
  cfg = config.cachix;
in
{
  options.cachix = {
    enable = lib.mkOption {
      type = lib.types.bool;
      description = "Whether to enable Cachix integration.";
      default = true;
    };

    pull = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = "Which Cachix caches to pull from.";
      default = [ ];
      defaultText = lib.literalExpression ''[ "devenv" ]'';
    };

    push = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Which Cachix cache to push to. This cache is also added to `cachix.pull`.";
      default = null;
    };

    package = lib.mkPackageOption pkgs "cachix" {
      default = "cachix";
      example = "inputs.devenv.inputs.cachix.packages.\${pkgs.stdenv.system}.cachix";
    };
  };

  config = lib.mkIf cfg.enable {
    cachix.pull = [ "devenv" ]
      ++ (lib.optional (cfg.push != null) config.cachix.push);

    warnings = lib.optionals (!config.devenv.flakesIntegration && lib.versionOlder config.devenv.cliVersion "1.0") [
      ''
        For cachix.push and cachix.pull attributes to have an effect,
        upgrade to devenv 1.0 or later.
      ''
    ];
  };
}
