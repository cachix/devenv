{ pkgs, ... }:

{
  packages = [ pkgs.curl pkgs.jq ];

  services.trafficserver = {
    enable = true;
    remap = "map / http://127.0.0.1 @plugin=generator.so";
    records.proxy.config = {
      proxy_name = "devenv.test";
      http.server_ports = "8080 8080:ipv6";

      diags.logfile.filename = "stdout";
      error.logfile.filename = "stderr";

      admin.user_id = "#-1";
    };
  };
}
