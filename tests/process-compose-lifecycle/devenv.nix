{ pkgs, ... }:

{
  packages = [ pkgs.curl pkgs.python3 ];

  process.manager.implementation = "process-compose";
  process.managers.process-compose.tui.enable = false;

  processes.http = {
    exec = "exec python3 -m http.server 18458";
    ready.http.get = {
      host = "127.0.0.1";
      port = 18458;
      path = "/";
      scheme = "http";
    };
    ready.probe_timeout = 3;
  };
}
