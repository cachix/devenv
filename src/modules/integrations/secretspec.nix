{ config, lib, pkgs, secretspec ? null, ... }:

let
  secretspecData =
    if secretspec != null then
      secretspec
    else
      let
        # The env var fallback is for flakes users who can't use the devenv CLI integration.
        envVar = builtins.getEnv "SECRETSPEC_SECRETS";
      in
      if envVar != "" then
        builtins.fromJSON envVar
      else
        null;
in
{
  options.secretspec = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = if secretspecData != null then true else false;
      readOnly = true;
      description = "Whether secretspec integration is enabled (automatically true when secrets are loaded)";
    };

    profile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = if secretspecData != null then secretspecData.profile else null;
      readOnly = true;
      description = "The secretspec profile that was used to load secrets (read-only)";
    };

    provider = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = if secretspecData != null then secretspecData.provider else null;
      readOnly = true;
      description = "The secretspec provider that was used to load secrets (read-only)";
    };

    secrets = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = if secretspecData != null then secretspecData.secrets else { };
      readOnly = true;
      description = "Secrets loaded from secretspec.toml (read-only)";
    };
  };

  config = {
    assertions = [
      {
        assertion = !(config.secretspec.enable && config.devenv.flakesIntegration);
        message = ''
          SecretSpec integration is not supported when using devenv with Nix Flakes.

          The devenv CLI is required to load secrets from secretspec.toml.
          See https://devenv.sh/integrations/secretspec/ for more information.
        '';
      }
    ];
  };
}
