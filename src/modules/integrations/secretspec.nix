{ config, lib, pkgs, secretspec ? null, ... }:

let
  # Use secretspec parameter if available,
  # otherwise fall back to SECRETSPEC_SECRETS environment variable (sigh, flakes)
  secretspecData =
    if secretspec != null then
      secretspec
    else
      let
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
}
