# Regression for #3137: process dependencies of `devenv tasks run` are owned by
# the manager started with `devenv up -d`. Task selection and dependency edges
# are the scheduler's own, but the processes are started by, judged against,
# and left running in that manager.
{ config, ... }:
let
  state = config.devenv.state;
  # Record every launch of a process so the test can tell reuse from restarts.
  countStart = name: ''
    mkdir -p ${state}
    echo $(( $(cat ${state}/${name}-starts 2>/dev/null || echo 0) + 1 )) > ${state}/${name}-starts
  '';
in
{
  process.manager.implementation = "native";

  # A healthy service started by `devenv up`.
  processes.dummy = {
    exec = ''
      ${countStart "dummy"}
      echo $$ > ${state}/dummy.pid
      exec sleep 3600
    '';
    ready = {
      exec = "test -f ${state}/dummy.pid";
      period = 1;
    };
  };

  # A finite worker: runs until released, then exits successfully.
  processes.worker.exec = ''
    ${countStart "worker"}
    while [ ! -e ${state}/worker-release ]; do sleep 0.1; done
  '';

  # Selected by test:wantedby without any ordering edge.
  processes.wanted = {
    exec = ''
      ${countStart "wanted"}
      exec sleep 3600
    '';
    ready.exec = "true";
  };
  tasks."devenv:processes:wanted".wantedBy = [ "test:wantedby" ];

  # Ordered after test:wantedby but opted out of selection by ordering edges.
  processes.optout = {
    exec = ''
      ${countStart "optout"}
      exec sleep 3600
    '';
    after = [ "test:wantedby" ];
  };
  tasks."devenv:processes:optout".wantedBy = [ ];

  tasks."test:repro" = {
    exec = ''
      kill -0 "$(cat ${state}/dummy.pid)"
      touch ${state}/repro-ran
    '';
    after = [ "devenv:processes:dummy" ];
  };

  tasks."test:wantedby".exec = "touch ${state}/wantedby-ran";

  tasks."test:completed" = {
    exec = "touch ${state}/completed-ran";
    after = [ "devenv:processes:worker@completed" ];
  };
}
