{ pkgs, lib, config, ... }:
let
  pcProcesses = config.process.managers.process-compose.settings.processes;
in
{
  process.manager.implementation = "process-compose";

  # An interactive process must be a direct child of the process-compose PTY.
  # Routing it through the `devenv-tasks` runner pipes its stdout/stderr and
  # breaks interactivity (no prompt, block-buffered output). Its generated
  # `command` must therefore be the raw `exec`, not the devenv-tasks wrapper.
  processes.repl = {
    exec = "python3";
    process-compose.is_interactive = true;
  };

  # A regular process must still be routed through `devenv-tasks` so that
  # task-dependency handling and env injection keep working.
  processes.web = {
    exec = "sleep infinity";
  };

  assertions = [
    {
      assertion = pcProcesses.repl.command == "python3";
      message = "interactive process should run `exec` directly, not via devenv-tasks. Got: ${pcProcesses.repl.command}";
    }
    {
      assertion = lib.hasInfix "devenv-tasks" pcProcesses.web.command;
      message = "non-interactive process should still route through devenv-tasks. Got: ${pcProcesses.web.command}";
    }
  ];

  enterTest = ''
    echo "interactive process command assertions passed"
  '';
}
