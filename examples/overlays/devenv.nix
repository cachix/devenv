{ pkgs, ... }:

{
  packages = [ pkgs.rust-bin.stable.latest.default ];
}
