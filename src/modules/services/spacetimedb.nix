{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.spacetimedb;

  basePort = cfg.port;
  allocatedPort = config.processes.spacetimedb.ports.main.value;

  stateDir = cfg.env.DEVENV_STATE + "/spacetimedb";
in
{
  options.services.spacetimedb = {
    enable = lib.mkEnableOption "SpacetimeDB Process";

    package = lib.mkPackageOption pkgs "spacetimedb" { };

    port = lib.mkOption {
      type = lib.types.port;
      default = 3000;
      description = "The TCP port to accept connection.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.rustc.llvmPackages.lld
      pkgs.binaryen
    ];

    processes.spacetimedb = {
      ports.main.allocate = basePort;
      exec = "${cfg.package}/bin/spacetime start --listen-addr 0.0.0.0:${toString allocatedPort} --data-dir ${stateDir}";
    };
  };
}
