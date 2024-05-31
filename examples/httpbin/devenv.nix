{ pkgs, ... }:

{
  packages = [ pkgs.curl ];

  services.httpbin = {
    enable = true;
    bind = [ "127.0.0.1:8080" "127.0.0.1:8081" ];
  };
}
