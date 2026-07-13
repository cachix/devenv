{ config, ... }:

{
  services.postgres.enable = true;

  assertions = [
    {
      assertion = config.processes.postgres.ports == { };
      message = "Socket-only PostgreSQL must not allocate a TCP port.";
    }
  ];
}
