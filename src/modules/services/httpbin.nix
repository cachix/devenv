{ pkgs, lib, config, ... }:

let
  cfg = config.services.httpbin;

  qs = lib.escapeShellArgs;

  # Port allocation: extract port from first bind address or use default
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  firstBind = lib.head cfg.bind;
  basePort = parsePort firstBind;
  allocatedPort = config.processes.httpbin.ports.main.value;
  host = parseHost firstBind;

  # Rebuild bind addresses with allocated port for first address
  allocatedBinds = [ "${host}:${toString allocatedPort}" ] ++ (lib.tail cfg.bind);

  python = pkgs.python3.withPackages (ps: with ps; [ httpbin gunicorn gevent ]);
  binds = lib.concatMap (addr: [ "-b" addr ]) allocatedBinds;
in
{
  options.services.httpbin = {
    enable = lib.mkEnableOption "httpbin";

    bind = lib.mkOption {
      type = with lib.types; listOf str;
      default = [ "127.0.0.1:8080" ];
      description = "Addresses for httpbin to listen on.";
    };

    extraArgs = lib.mkOption {
      type = with lib.types; listOf str;
      default = [ ];
      description = "Gunicorn CLI arguments for httpbin.";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.httpbin.ports.main.allocate = basePort;
    processes.httpbin.exec = "exec ${python}/bin/gunicorn httpbin:app -k gevent ${qs binds} ${qs cfg.extraArgs}";
  };
}

