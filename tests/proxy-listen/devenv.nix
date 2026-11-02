{ config, pkgs, ... }:
let
  listeners = import ./listeners.nix;
in
{
  name = listeners.name;
  packages = [
    pkgs.python3
    pkgs.curl
    pkgs.mkcert
  ];
  process.manager.implementation = "native";
  process.proxy = {
    enable = listeners.enable or true;
    listen = listeners.http;
    https.listen = listeners.https or "127.0.0.1:443";
  };
  processes.web = {
    exec = "${pkgs.python3}/bin/python3 -m http.server ${toString config.processes.web.ports.http.value} --bind 127.0.0.1";
    ports.http.allocate = 18130;
    proxy.https.enable = listeners.tls or false;
  };
}
