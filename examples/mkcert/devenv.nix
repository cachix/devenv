{ pkgs, config, ... }:

{
  certificates = [
    "example.com"
    "another-example.com"
  ];

  hosts = {
    "example.com" = "127.0.0.1";
    "another-example.com" = [ "127.0.0.1" "::1" ];
  };

  services.caddy.enable = true;
  services.caddy.virtualHosts."example.com" = {
    extraConfig = ''
      tls ${config.env.DEVENV_STATE}/mkcert/example.com.pem ${config.env.DEVENV_STATE}/mkcert/example.com-key.pem

      respond "Hello, world from example.com!"
    '';
  };
  services.caddy.virtualHosts."another-example.com" = {
    extraConfig = ''
      tls ${config.env.DEVENV_STATE}/mkcert/another-example.com.pem ${config.env.DEVENV_STATE}/mkcert/another-example.com-key.pem

      respond "Hello, world from another-example.com!"
    '';
  };
}
