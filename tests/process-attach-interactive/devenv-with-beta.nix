{ pkgs, ... }:
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";

  processes.alpha = {
    exec = "exec python3 -u -m http.server 18641";
    ready.exec = "curl -sf -o /dev/null http://127.0.0.1:18641/";
  };
  processes.beta = {
    exec = "exec python3 -u -m http.server 18642";
    start.enable = false;
  };
}
