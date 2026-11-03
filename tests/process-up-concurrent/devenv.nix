{ config, pkgs, ... }:
{
  packages = [ pkgs.python3 ];
  process.manager.implementation = "native";
  processes.slow = {
    exec = ''
      echo $$ >> "${config.devenv.state}/launch-pids"
      exec sleep infinity
    '';
    ready.exec = "test -e ${config.devenv.state}/ready";
  };
}
