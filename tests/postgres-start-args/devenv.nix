{ lib, ... }:

{
  services.postgres.enable = true;

  processes.postgres.exec = lib.mkForce "exec start-postgres -c max_connections=42";
}
