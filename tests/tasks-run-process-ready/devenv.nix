{ config, ... }:
{
  # Process with an exec readiness probe. The probe is spawned via the bash
  # path resolved into the task config; when that path is unset the probe
  # cannot spawn and the @ready edge below never fires.
  #
  # The payload is backgrounded rather than `exec`ed so it is a grandchild of
  # devenv, the way a service that forks (postgres, redis) is. Tokio kills the
  # direct child on drop, so an `exec`ed payload would be cleaned up by luck
  # even when the manager never tore the process down.
  processes.probe-target = {
    exec = ''
      mkdir -p ${config.devenv.state}
      sleep infinity &
      echo $! > ${config.devenv.state}/probe-target.pid
      touch ${config.devenv.state}/probe-target-ready
      wait
    '';
    ready.exec = "test -f ${config.devenv.state}/probe-target-ready";
  };

  # `after` on a process defaults to @ready, so this task only runs once the
  # readiness probe above has succeeded.
  tasks."test:after-ready" = {
    after = [ "devenv:processes:probe-target" ];
    exec = ''
      mkdir -p ${config.devenv.state}
      touch ${config.devenv.state}/after-ready-ran
    '';
  };
}
