# Exercises daemon pid-file/socket ownership: a second `up -d` attaches
# instead of clobbering the daemon's runtime files, a non-interactive
# foreground `up` fails fast, `down` stops the daemon without orphaning its
# children, and `down` is idempotent.
{ config, pkgs, ... }:
let
  httpPort = config.processes.http.ports.main.value;
in
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";
  processes.http = {
    exec = ''
      printf '%s\n' ${toString httpPort} > "${config.devenv.state}/http-port"
      exec python3 -m http.server ${toString httpPort}
    '';
    ports.main.allocate = 18457;
  };
  tasks."test:after-up" = {
    after = [ "devenv:up@stopped" ];
    exec = ''
      if curl --fail --silent --max-time 1 http://127.0.0.1:${toString httpPort}/ >/dev/null; then
        echo "HTTP process was still running during cleanup" >&2
        exit 1
      fi
      printf 'stopped\n' >> "${config.devenv.state}/up-stopped"
    '';
  };
}
