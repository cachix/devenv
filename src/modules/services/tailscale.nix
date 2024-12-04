{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.tailscale;
in
{
  options = {
    services.tailscale = {
      funnel = {
        enable = mkEnableOption "Tailscale funnel";

        target = mkOption {
          type = types.str;
          description = "Target host or host:port for Tailscale funnel";
        };
      };
    };
  };

  config.processes = lib.mkIf cfg.funnel.enable {
    "tailscale-funnel" = {
      exec = "${pkgs.tailscale}/bin/tailscale funnel --yes ${cfg.funnel.target}";
    };
  };
}
