{ pkgs, ... }:

{
  packages = [ pkgs.rust-bin.stable.latest.default ];

  services.blackfire.enable = true;
}
