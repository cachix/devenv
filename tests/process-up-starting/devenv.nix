# A foreground `devenv up` whose processes are still starting must already be
# visible to other `devenv up` calls.
{ config, ... }:
{
  process.manager.implementation = "native";
  tasks."test:setup".exec = ''
    if test -e "${config.devenv.state}/block-setup"; then
      echo $$ > "${config.devenv.state}/setup-pid"
      exec sleep infinity
    fi
  '';
  tasks."test:startup-failure" = {
    after = [ "devenv:processes:slow@started" ];
    exec = ''
      if test -e "${config.devenv.state}/fail-startup"; then
        touch "${config.devenv.state}/failure-task-started"
        while ! test -e "${config.devenv.state}/fail-now"; do sleep 0.1; done
        echo "INTENTIONAL_STARTUP_FAILURE" >&2
        exit 23
      fi
    '';
  };
  processes.dependent = {
    after = [ "test:startup-failure" ];
    exec = "exec sleep infinity";
  };
  processes.slow = {
    after = [ "test:setup" ];
    exec = ''
      echo $$ > "${config.devenv.state}/slow-pid"
      touch "${config.devenv.state}/slow-started"
      exec sleep infinity
    '';
    # Not ready until the test creates this file.
    ready.exec = "test -e ${config.devenv.state}/slow-ready";
  };
}
