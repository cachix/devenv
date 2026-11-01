# A foreground `devenv up` whose processes are still starting must already be
# visible to other `devenv up` calls.
{ config, ... }:
{
  process.manager.implementation = "native";
  processes.slow = {
    exec = ''
      touch "${config.devenv.state}/slow-started"
      exec sleep infinity
    '';
    # Not ready until the test creates this file.
    ready.exec = "test -e ${config.devenv.state}/slow-ready";
  };
}
