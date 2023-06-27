{ pkgs, ... }:

{
  services.clickhouse = {
    enable = true;
  };

  # for the tests
  packages = [ pkgs.coreutils-full ];
}
