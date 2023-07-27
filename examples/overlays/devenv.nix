{ pkgs, ... }:

{
  packages = [
    # from the rust-overlay
    pkgs.rust-bin.stable.latest.default

    # from subflake
    pkgs.hello2
  ];
}
