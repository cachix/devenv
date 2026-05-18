# Test that a foreground `devenv up` refuses to start when a daemon is already
# running, and that `devenv processes down` correctly stops the daemon without
# leaving orphaned child processes.
{ pkgs, ... }:
{
  packages = [ pkgs.python3 pkgs.curl ];
  process.manager.implementation = "native";
  processes.http.exec = "exec python3 -m http.server 18457";
}
