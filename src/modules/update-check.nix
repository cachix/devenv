{ lib, config, ... }:

let
  cfg = config.devenv;
in
{
  options.devenv = {
    flakesIntegration = lib.mkOption {
      type = lib.types.bool;
      default = false;
      defaultText = lib.literalMD ''`true` when devenv is invoked via the flake integration; `false` otherwise.'';
      description = ''
        Tells if devenv is being imported by a flake.nix file
      '';
    };
    warnOnNewVersion = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to warn when a new version of either devenv or the direnv integration is available.
      '';
    };
    cliVersion = lib.mkOption {
      type = lib.types.str;
      internal = true;
    };
    latestVersion = lib.mkOption {
      type = lib.types.str;
      default = lib.fileContents ./latest-version;
      description = ''
        The latest version of devenv.
      '';
    };
    direnvrcLatestVersion = lib.mkOption {
      type = lib.types.int;
      description = ''
        The latest version of the direnv integration.
      '';
      internal = true;
      default = 1;
    };
  };

  config = lib.mkIf cfg.warnOnNewVersion {
    enterShell =
      let
        action = {
          "0" = "";
          "1" = ''
            echo "✨ devenv ${cfg.cliVersion} is newer than devenv input (${cfg.latestVersion}) in devenv.lock. Run 'devenv update' to sync." >&2
          '';
          "-1" = ''
            echo "✨ devenv ${cfg.cliVersion} is out of date. Please update to ${cfg.latestVersion}: https://devenv.sh/getting-started/#installation" >&2
          '';
        };
      in
      ''
        # Check whether a newer version of the devenv CLI is available.
        ${action."${toString (builtins.compareVersions cfg.cliVersion cfg.latestVersion)}"}

        # Check whether the direnv integration is out of date.
        {
          if [[ ":''${DIRENV_ACTIVE-}:" == *":${cfg.root}:"* ]]; then
            if [[ ! "''${DEVENV_NO_DIRENVRC_OUTDATED_WARNING-}" == 1 && ! "''${DEVENV_DIRENVRC_ROLLING_UPGRADE-}" == 1 ]]; then
              if [[ ''${DEVENV_DIRENVRC_VERSION:-0} -lt ${toString cfg.direnvrcLatestVersion} ]]; then
                direnv_line=$(grep --color=never -E "source_url.*cachix/devenv" .envrc || echo "")

                echo "✨ The direnv integration in your .envrc is out of date."
                echo ""
                echo -n "RECOMMENDED: devenv can now auto-upgrade the direnv integration. "
                if [[ -n "$direnv_line" ]]; then
                  echo "To enable this feature, replace the following line in your .envrc:"
                  echo ""
                  echo "  $direnv_line"
                  echo ""
                  echo "with:"
                  echo ""
                  echo "  eval \"\$(devenv direnvrc)\""
                else
                  echo "To enable this feature, replace the \`source_url\` line that fetches the direnvrc integration in your .envrc with:"
                  echo ""
                  echo "  eval \"$(devenv direnvrc)\""
                fi
                echo ""
                  echo "If you prefer to continue managing the integration manually, follow the upgrade instructions at https://devenv.sh/automatic-shell-activation/."
                  echo ""
                  echo "To disable this message:"
                  echo ""
                  echo "  Add the following environment to your .envrc before \`use devenv\`:"
                  echo ""
                  echo "    export DEVENV_NO_DIRENVRC_OUTDATED_WARNING=1"
                  echo ""
                  echo "  Or set the following option in your devenv configuration:"
                  echo ""
                  echo "    devenv.warnOnNewVersion = false;"
                  echo ""
              fi
            fi
          fi
        } >&2
      '';
  };
}
