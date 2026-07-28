{ pkgs, ... }:
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "process-compose";

  processes.alpha.exec = "exec python3 -u -m http.server 18661";
}
