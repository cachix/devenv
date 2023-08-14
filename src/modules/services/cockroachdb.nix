{ pkgs, lib, config, ... }:

let
  cfg = config.services.cockroachdb;
  types = lib.types;
in
{
  options.services.cockroachdb = {
    enable = lib.mkEnableOption ''
      Add CockroachDB process.
    '';

    listen_addr = lib.mkOption {
      type = types.str;
      default = "localhost:26257";
      description = ''
        The address/hostname and port to listen on.
      '';
    };

    http_addr = lib.mkOption {
      type = types.str;
      default = "localhost:8080";
      description = ''
        The hostname or IP address to bind to for HTTP requests.
      '';
    };

    package = lib.mkOption {
      default = pkgs.cockroachdb-bin;
      defaultText = "pkgs.cockroachdb-bin";
      description = "The CockroachDB package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env.COCKROACH_DATA = config.env.DEVENV_STATE + "/cockroachdb";

    processes.cockroachdb = {
      exec = "${cfg.package}/bin/cockroachdb start-single-node --insecure --listen-addr=${cfg.listen_addr} --http-addr=${cfg.http_addr} --store=path=$COCKROACH_DATA";
    };
  };
}
