{ pkgs, lib, ... }:

{
  options.devenv = {
    warnOnNewVersion = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to warn when a new version of devenv is available.
      '';
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
    enterShell = ''
      if [ "$DEVENV_VERSION" != "${cfg.latestVersion}" ]; then
        echo "✨ devenv is out of date. Please update to ${cfg.latestVersion}: https://devenv.sh/getting-started/#installation" >&2
      fi
    '';
  };
}
