{ pkgs, ... }:
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";

  processes.alpha.exec = "exec python3 -u -m http.server 18641";
}
