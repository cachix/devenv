# Test that a second `devenv up` attaches to the already-running native manager
# (over its control socket) and (re)starts the up-enabled processes, instead of
# failing with "Processes already running".
{ pkgs, ... }:
{
  packages = [ pkgs.python3 pkgs.curl ];
  process.manager.implementation = "native";

  processes.alpha.exec = "exec python3 -m http.server 18561";
  processes.beta.exec = "exec python3 -m http.server 18562";
}
