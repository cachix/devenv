{ pkgs, ... }:

{
  packages = [
    pkgs.influxdb
  ];

  services.influxdb.enable = true;
}
