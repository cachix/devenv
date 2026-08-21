{ config, ... }:
{
  # The exec probe covers #3030; the background child exposes orphan cleanup.
  processes.probe-target = {
    exec = ''
      mkdir -p ${config.devenv.state}
      rm -f ${config.devenv.state}/probe-target.hold
      mkfifo ${config.devenv.state}/probe-target.hold
      cat ${config.devenv.state}/probe-target.hold >/dev/null &
      echo $! > ${config.devenv.state}/probe-target.pid
      touch ${config.devenv.state}/probe-target-started
      touch ${config.devenv.state}/probe-target-ready
      wait
    '';
    ready = {
      exec = "test -f ${config.devenv.state}/probe-target-ready";
      period = 1;
    };
  };

  # Unqualified process dependencies default to @ready.
  tasks."test:after-ready" = {
    after = [ "devenv:processes:probe-target" ];
    exec = ''
      mkdir -p ${config.devenv.state}
      if [ ! -f ${config.devenv.state}/probe-target-ready ]; then
        touch ${config.devenv.state}/success-ordering-violation
        exit 21
      fi
      touch ${config.devenv.state}/after-ready-ran
    '';
  };

  # #2037: selecting this root must include the full mixed predecessor closure.
  processes.chain-backend = {
    after = [ "test:after-ready@succeeded" ];
    exec = ''
      mkdir -p ${config.devenv.state}
      if [ ! -f ${config.devenv.state}/after-ready-ran ]; then
        touch ${config.devenv.state}/success-ordering-violation
        exit 22
      fi
      touch ${config.devenv.state}/chain-backend-started
      rm -f ${config.devenv.state}/chain-backend.hold
      mkfifo ${config.devenv.state}/chain-backend.hold
      cat ${config.devenv.state}/chain-backend.hold >/dev/null &
      echo $! > ${config.devenv.state}/chain-backend.pid
      wait
    '';
    ready = {
      exec = "test -f ${config.devenv.state}/chain-backend-started";
      period = 1;
    };
  };

  processes.unrelated.exec = ''
    mkdir -p ${config.devenv.state}
    touch ${config.devenv.state}/unrelated-started
    rm -f ${config.devenv.state}/unrelated.hold
    mkfifo ${config.devenv.state}/unrelated.hold
    read _ < ${config.devenv.state}/unrelated.hold
  '';

  # A failed bridge must block the downstream process.
  processes.failure-source = {
    exec = ''
      mkdir -p ${config.devenv.state}
      rm -f ${config.devenv.state}/failure-source.hold
      mkfifo ${config.devenv.state}/failure-source.hold
      cat ${config.devenv.state}/failure-source.hold >/dev/null &
      echo $! > ${config.devenv.state}/failure-source.pid
      touch ${config.devenv.state}/failure-source-ready
      wait
    '';
    ready = {
      exec = "test -f ${config.devenv.state}/failure-source-ready";
      period = 1;
    };
  };

  tasks."test:failing-bridge" = {
    after = [ "devenv:processes:failure-source@ready" ];
    exec = ''
      touch ${config.devenv.state}/failing-bridge-ran
      exit 23
    '';
  };

  processes.blocked-backend = {
    after = [ "test:failing-bridge@succeeded" ];
    exec = ''
      touch ${config.devenv.state}/blocked-backend-started
      rm -f ${config.devenv.state}/blocked-backend.hold
      mkfifo ${config.devenv.state}/blocked-backend.hold
      read _ < ${config.devenv.state}/blocked-backend.hold
    '';
  };
}
