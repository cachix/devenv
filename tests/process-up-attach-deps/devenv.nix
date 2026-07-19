# Test that an attaching `devenv up` honours `after`/`before` dependencies via
# the daemon's task scheduler, even when the dependency is not in the requested
# subset (it is the daemon — not the CLI — that orders the starts).
#
# beta depends on gamma being ready. When gamma is stopped, attaching
# `devenv up beta` must NOT launch beta (its dependency is unmet); once gamma is
# started, beta follows automatically.
{ pkgs, ... }:
{
  packages = [
    pkgs.python3
    pkgs.curl
  ];
  process.manager.implementation = "native";

  processes.alpha.exec = "exec python3 -m http.server 18581";

  processes.gamma = {
    exec = "exec python3 -m http.server 18583";
    # Ready once the port answers, so beta's `@ready` dependency is well-defined.
    ready.exec = "curl -sf -o /dev/null http://127.0.0.1:18583/";
  };

  processes.beta = {
    exec = "exec python3 -m http.server 18582";
    after = [ "devenv:processes:gamma@ready" ];
  };
}
