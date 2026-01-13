{ pkgs, lib, config, ... }:

let
  cfg = config.services.cockroachdb;
  types = lib.types;

  # Port allocation: extract port from address strings
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  baseListenPort = parsePort cfg.listen_addr;
  baseHttpPort = parsePort cfg.http_addr;
  allocatedListenPort = config.processes.cockroachdb.ports.main.value;
  allocatedHttpPort = config.processes.cockroachdb.ports.http.value;
  listenHost = parseHost cfg.listen_addr;
  httpHost = parseHost cfg.http_addr;
  listenAddr = "${listenHost}:${toString allocatedListenPort}";
  httpAddr = "${httpHost}:${toString allocatedHttpPort}";
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
      default = pkgs.cockroachdb;
      defaultText = lib.literalExpression "pkgs.cockroachdb";
      description = "The CockroachDB package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env.COCKROACH_DATA = config.env.DEVENV_STATE + "/cockroachdb";

    processes.cockroachdb = {
      ports.main.allocate = baseListenPort;
      ports.http.allocate = baseHttpPort;
      exec = "exec ${cfg.package}/bin/cockroachdb start-single-node --insecure --listen-addr=${listenAddr} --http-addr=${httpAddr} --store=path=$COCKROACH_DATA";
    };
  };
}
