{ pkgs, ... }:

{
  packages = [ pkgs.curl ];

  services.rustfs.enable = true;
}
