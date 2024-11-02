{ pkgs, lib, config, ... }:

let
  cfg = config.services.httpbin;

  qs = lib.escapeShellArgs;

  python = pkgs.python3.withPackages (ps: with ps; [ httpbin gunicorn gevent ]);
  binds = lib.concatMap (addr: [ "-b" addr ]) cfg.bind;
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
    processes.httpbin.exec = ''
      exec ${python}/bin/gunicorn httpbin:app -k gevent ${qs binds} ${qs cfg.extraArgs}
    '';
  };
}

