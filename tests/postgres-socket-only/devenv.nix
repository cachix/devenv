{ config, ... }:

{
  services.postgres.enable = true;

  assertions = [
    {
      assertion = config.processes.postgres.ports == { };
      message = "Socket-only PostgreSQL must not allocate a TCP port.";
    }
    {
      assertion = config.processes.postgres.shutdown.signal == 2;
      message = "PostgreSQL must use SIGINT for fast shutdown.";
    }
  ];
}
