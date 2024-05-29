{ pkgs, ... }:

{
  packages = [ pkgs.curl ];

  services.trafficserver = {
    enable = true;
    remap = "map / http://127.0.0.1 @plugin=generator.so";
    records.proxy.config.http.server_ports = "8080 8080:ipv6";
  };
}
