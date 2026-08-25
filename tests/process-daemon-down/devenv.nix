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
}
