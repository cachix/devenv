# `start.shell = true` requires the native process manager; using it with
# process-compose must fail with a clear assertion error.
{ pkgs, ... }:
{
  process.manager.implementation = "process-compose";
  processes.web = {
    exec = "exec sleep 1000";
    start.shell = true;
  };
}
