{ pkgs, config, ... }:
let
  ports = map (port: port.value) (builtins.attrValues config.processes.httpbin.ports);
in
{
  packages = [ pkgs.curl ];

  services.httpbin = {
    enable = true;
    bind = [
      "127.0.0.1:8080"
      "127.0.0.1:8081"
    ];
  };

  enterTest = ''
    export HTTPBIN_PORTS="${toString ports}"
  '';
}
