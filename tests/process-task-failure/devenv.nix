{ config, ... }:

let
  markerDir = "${config.devenv.state}/process-task-failure";
in
{
  processes.server = {
    exec = ''
      mkdir -p ${markerDir}
      sleep 300 &
      echo "$!" > ${markerDir}/server.pid
      touch ${markerDir}/server.ready
      wait
    '';
    ready.exec = "test -f ${markerDir}/server.ready";
    restart.on = "never";
  };

  tasks."test:prerequisite" = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/prerequisite.ran
      echo "PROCESS_TASK_PREREQUISITE_RAN"
    '';
  };

  tasks."test:failure" = {
    exec = ''
      if [ ! -f ${markerDir}/prerequisite.ran ]; then
        echo "PROCESS_TASK_PREREQUISITE_MISSING"
        exit 42
      fi

      echo "INTENTIONAL_PROCESS_TASK_FAILURE"
      exit 23
    '';
    after = [
      "test:prerequisite"
      "devenv:processes:server"
    ];
  };
}
