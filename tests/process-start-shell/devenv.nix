# Test `start.shell` and `start.up` per-process flags and global defaults.
#
# - `start.up = true` (default): process starts on `devenv up`.
# - `start.shell = true`: process starts on interactive shell entry only.
# - `start.up = false`: process never starts automatically.
# - `process.shellStartProcesses` lists only the `start.shell = true` processes.
{ pkgs, ... }:
{
  packages = [ pkgs.python3 pkgs.curl ];
  process.manager.implementation = "native";

  processes.up_proc.exec = "exec python3 -m http.server 18551";
  processes.shell_proc = {
    exec = "exec python3 -m http.server 18552";
    start.shell = true;
    start.up = false;
  };
  processes.off_proc = {
    exec = "exec python3 -m http.server 18553";
    start.up = false;
  };
}
