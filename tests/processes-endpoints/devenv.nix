{ config, lib, pkgs, ... }:

{
  process.manager.implementation = "native";

  processes.app-1 = {
    ports.http.allocate = 18080;
    urls.web = {
      scheme = "http";
      host = "127.0.0.1";
      port = config.processes.app-1.ports.http.value;
      path = "/";
    };
    exec = ''
      exec ${lib.getExe pkgs.python3} -m http.server ${toString config.processes.app-1.ports.http.value}
    '';
    ready.http.get = {
      port = config.processes.app-1.ports.http.value;
      path = "/";
    };
  };

  processes.app-2 = {
    ports.http.allocate = 18080;
    urls.web = {
      scheme = "http";
      host = "127.0.0.1";
      port = config.processes.app-2.ports.http.value;
      path = "/";
    };
    exec = ''
      exec ${lib.getExe pkgs.python3} -m http.server ${toString config.processes.app-2.ports.http.value}
    '';
    ready.http.get = {
      port = config.processes.app-2.ports.http.value;
      path = "/";
    };
  };
}
