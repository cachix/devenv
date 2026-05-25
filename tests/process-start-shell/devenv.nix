# Test the `start.enable = "shell"` value and the global `process.start` default.
#
# - `start.enable = "shell"` processes are still started by `devenv up` (they are
#   enabled; "shell" only adds the shell-entry trigger).
# - `start.enable = false` processes are not started by `devenv up`.
# - `process.shellStartProcesses` lists only the "shell" processes.
{ pkgs, ... }:
{
  packages = [ pkgs.python3 pkgs.curl ];
  process.manager.implementation = "native";

  processes.up_proc.exec = "exec python3 -m http.server 18551";
  processes.shell_proc = {
    exec = "exec python3 -m http.server 18552";
    start.enable = "shell";
  };
  processes.off_proc = {
    exec = "exec python3 -m http.server 18553";
    start.enable = false;
  };
}
