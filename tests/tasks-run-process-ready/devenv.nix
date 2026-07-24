{ config, ... }:
{
  # Process with an exec readiness probe. The probe is spawned via the bash
  # path resolved into the task config; when that path is unset the probe
  # cannot spawn and the @ready edge below never fires.
  processes.probe-target = {
    exec = ''
      mkdir -p ${config.devenv.state}
      touch ${config.devenv.state}/probe-target-ready
      exec sleep infinity
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
