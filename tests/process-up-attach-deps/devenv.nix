# Test that an attaching `devenv up` honours `after`/`before` dependencies via
# the daemon's task scheduler, even when the dependency is not in the requested
# subset (it is the daemon — not the CLI — that orders the starts).
#
# beta depends on gamma being ready. When gamma is stopped, attaching
# `devenv up beta` must NOT launch beta (its dependency is unmet); once gamma is
# started, beta follows automatically.
{ config, pkgs, ... }:
let
  httpServer =
    name: basePort:
    let
      port = toString config.processes.${name}.ports.main.value;
    in
    {
      ports.main.allocate = basePort;
      exec = ''
        printf '%s\n' ${port} > "${config.devenv.state}/${name}-port"
        exec python3 -m http.server ${port}
      '';
    };
  gammaPort = toString config.processes.gamma.ports.main.value;
in
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";

  processes.alpha = httpServer "alpha" 18581;

  processes.gamma = httpServer "gamma" 18583 // {
    # Ready once the port answers, so beta's `@ready` dependency is well-defined.
    ready.exec = "curl -sf -o /dev/null http://127.0.0.1:${gammaPort}/";
  };

  processes.beta = httpServer "beta" 18582 // {
    after = [ "devenv:processes:gamma@ready" ];
  };
}
