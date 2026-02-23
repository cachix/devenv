{ pkgs, config, ... }:

{
  processes.server = {
    exec = "${pkgs.python3}/bin/python -m http.server ${toString config.processes.server.ports.http.value}";
    ports.http.allocate = 8080;
    ready.http.get = {
      port = config.processes.server.ports.http.value;
      path = "/";
    };
    restart.on = "on_failure";
  };

  processes.worker = {
    exec = ''
      echo "server is ready, starting worker"
      exec sleep infinity
    '';
    after = [ "devenv:processes:server" ];
  };
}
