# Regression for #3137: a task that depends on a process must reuse the
# already-running manager from `devenv up -d` instead of starting a second copy.
{ config, ... }:
{
  process.manager.implementation = "native";

  processes.dummy = {
    exec = ''
      mkdir -p ${config.devenv.state}
      starts=${config.devenv.state}/dummy-starts
      echo $(( $(cat "$starts" 2>/dev/null || echo 0) + 1 )) > "$starts"
      echo $$ > ${config.devenv.state}/dummy.pid
      touch ${config.devenv.state}/dummy-ready
      exec sleep 3600
    '';
    ready = {
      exec = "test -f ${config.devenv.state}/dummy-ready";
      period = 1;
    };
  };

  tasks."test:repro" = {
    exec = ''
      test -f ${config.devenv.state}/dummy.pid
      kill -0 "$(cat ${config.devenv.state}/dummy.pid)"
      echo ran > ${config.devenv.state}/repro-ran
    '';
    after = [ "devenv:processes:dummy" ];
  };
}
