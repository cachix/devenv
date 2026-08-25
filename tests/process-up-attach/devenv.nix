# Test that a second `devenv up` attaches to the already-running native manager
# (over its control socket) and (re)starts the up-enabled processes, instead of
# failing with "Processes already running".
{ config, pkgs, ... }:
let
  alphaPort = config.processes.alpha.ports.main.value;
  betaPort = config.processes.beta.ports.main.value;
in
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";

  processes.alpha = {
    exec = ''
      printf '%s\n' ${toString alphaPort} > "${config.devenv.state}/alpha-port"
      exec python3 -m http.server ${toString alphaPort}
    '';
    ports.main.allocate = 18561;
  };
  processes.beta = {
    exec = ''
      printf '%s\n' ${toString betaPort} > "${config.devenv.state}/beta-port"
      exec python3 -m http.server ${toString betaPort}
    '';
    ports.main.allocate = 18562;
  };
}
