{ pkgs, lib, config, ... }:

let
  cfg = config.services.httpbin;

  qs = lib.escapeShellArgs;

  parseBind = addr:
    if lib.hasPrefix "unix:" addr then null
    else builtins.match "(.*):([0-9]+)" addr;

  tcpListeners = lib.imap0
    (index: addr:
      let parts = parseBind addr;
      in {
        inherit addr;
        name = if index == 0 then "main" else "bind-${toString index}";
        host = builtins.head parts;
        basePort = lib.toInt (builtins.elemAt parts 1);
      })
    (lib.filter (addr: parseBind addr != null) cfg.bind);

  allocatedBinds =
    let
      replacements = lib.listToAttrs (map
        (listener: {
          name = listener.addr;
          value = "${listener.host}:${toString config.processes.httpbin.ports.${listener.name}.value}";
        })
        tcpListeners);
    in
    map (addr: replacements.${addr} or addr) cfg.bind;

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
    changelogs = [
      {
        date = "2026-10-26";
        title = "HTTPBin allocates ports for every TCP listener";
        when = builtins.length tcpListeners > 1;
        description = ''
          All TCP addresses in `services.httpbin.bind` now participate in port allocation.
          Previously, only the first listener was allocated; additional listeners used fixed ports.
          If a requested port is occupied, devenv selects another available port.

          Use `config.processes.httpbin.ports.main.value` for the first TCP listener and
          `config.processes.httpbin.ports.bind-1.value`, `bind-2.value`, and so on for subsequent
          TCP listeners when constructing client URLs.
          With the native process manager running, `devenv processes list` shows the assigned ports.
        '';
      }
    ];

    processes.httpbin.ports = lib.listToAttrs (map
      (listener: lib.nameValuePair listener.name { allocate = listener.basePort; })
      tcpListeners);
    processes.httpbin.exec = "exec ${python}/bin/gunicorn httpbin:app -k gevent ${qs binds} ${qs cfg.extraArgs}";
  };
}
