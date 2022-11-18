{ pkgs, lib, config, ... }:

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

  config = lib.mkIf config.devenv.warnOnNewVersion {
    enterShell = ''
      if [ "$DEVENV_VERSION" != "${config.devenv.latestVersion}" ]; then
        echo "✨ devenv is out of date. Please update to ${config.devenv.latestVersion}: https://devenv.sh/getting-started/#installation" >&2
      fi
    '';
  };
}
