{ config, pkgs, ... }:
let
  alphaPort = toString config.processes.alpha.ports.main.value;
in
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";

  processes.alpha = {
    ports.main.allocate = 18641;
    exec = ''
      printf '%s\n' ${alphaPort} > "$DEVENV_STATE/alpha-port"
      exec python3 -u -m http.server ${alphaPort}
    '';
    ready.exec = "curl -sf -o /dev/null http://127.0.0.1:${alphaPort}/";
  };
}
