{
  pkgs,
  config,
  lib,
  ...
}:
let
  cachePort = toString config.processes.cache.ports.http.value;
in
{
  packages = [
    pkgs.nix
    pkgs.gnutar
    pkgs.gzip
  ];

  # Use HTTP because the Linux runner and nix-daemon have separate /tmp namespaces.
  processes.cache = {
    ports.http.allocate = 18881;
    exec = ''
      exec ${lib.getExe pkgs.python3Minimal} -m http.server ${cachePort} \
        --bind 127.0.0.1 --directory "$DEVENV_STATE/locked-input-cache"
    '';
  };

  enterTest = ''
    export CACHE_URL="http://127.0.0.1:${cachePort}"
    wait_for_port ${cachePort}
  '';
}
