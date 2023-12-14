{ pkgs, ... }:
{
  services.varnish = {
    enable = true;
    package = pkgs.varnish;
    vcl = ''
      vcl 4.0;

      backend default {
        .host = "127.0.0.1";
        .port = "8001";
      }
    '';
  };

  services.caddy.enable = true;
  services.caddy.virtualHosts.":8001" = {
    extraConfig = ''
      respond "Hello, world!"
    '';
  };
}
