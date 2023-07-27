{ lib, config, ... }:
let
  cfg = config.cachix;
in
{
  options.cachix = {
    pull = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = "What caches to pull from.";
    };

    push = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "What cache to push to. Automatically also adds it to the list of caches to pull from.";
      default = null;
    };
  };

  config = {
    cachix.pull = [ "devenv" ]
      ++ (lib.optionals (cfg.push != null) [ config.cachix.push ]);

    warnings = lib.optionals (!config.devenv.flakesIntegration && lib.versionOlder config.devenv.cliVersion "1.0") [
      ''
        For cachix.push and cachix.pull attributes to have an effect,
        upgrade to devenv 1.0 or later.
      ''
    ];
  };
}
