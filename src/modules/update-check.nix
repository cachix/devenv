{ pkgs, lib, config, ... }:

let
  cfg = config.devenv;
in
{
  options.devenv = {
    warnOnNewVersion = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to warn when a new version of devenv is available.
      '';
    };
    cliVersion = lib.mkOption {
      type = lib.types.str;
      default = "0.3";
      internal = true;
    };
    latestVersion = lib.mkOption {
      type = lib.types.str;
      default = lib.removeSuffix "\n" (builtins.readFile ./latest-version);
      description = ''
        The latest version of devenv.
      '';
    };
  };

  config = lib.mkIf cfg.warnOnNewVersion {
    enterShell = lib.optionalString (cfg.cliVersion != cfg.latestVersion) ''
      echo "✨ devenv ${cfg.cliVersion} is out of date. Please update to ${cfg.latestVersion}: https://devenv.sh/getting-started/#installation" >&2
    '';
  };
}
