{ pkgs, lib, config, ... }:

let
  cfg = config.devenv;
  action = {
    "0" = "";
    "1" = ''
      echo "✨ devenv ${cfg.cliVersion} is newer than devenv input in devenv.lock. Run `devenv update` to sync.
    '';
    "-1" = ''
      echo "✨ devenv ${cfg.cliVersion} is out of date. Please update to ${cfg.latestVersion}: https://devenv.sh/getting-started/#installation" >&2
    '';
  };
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
    enterShell = action."${ toString (builtins.compareVersions cfg.cliVersion cfg.latestVersion) }";
  };
}
