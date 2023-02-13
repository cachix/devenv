{ pkgs, config, ... }:

{
  mkcert.domains = [
    "foo.de"
  ];

  services.caddy.enable = true;
}
