{ pkgs, config, ... }:

{
  certificates = [
    "example.com"
  ];

  hosts = {
    "example.com" = "127.0.0.1";
  };

  services.caddy.enable = true;
  services.caddy.virtualHosts."example.com" = {
    extraConfig = ''
      tls ${config.env.DEVENV_STATE}/mkcert/example.com.pem ${config.env.DEVENV_STATE}/mkcert/example.com-key.pem

      respond "Hello, world!"
    '';
  };
}
