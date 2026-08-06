{ pkgs, lib, config, ... }:
let
  pcProcesses = config.process.managers.process-compose.settings.processes;
  waitForSettledProcesses = pkgs.writeShellScript "wait-for-settled-processes" ''
    while true; do
      output=$(process-compose list --output json 2>/dev/null) || {
        sleep 0.1
        continue
      }
      completed_code=$(jq -r '.[] | select(.name == "completed") | .exit_code' <<<"$output")
      failed_code=$(jq -r '.[] | select(.name == "failed") | .exit_code' <<<"$output")
      if test "$completed_code" = 0 &&
        test "$failed_code" != null && test "$failed_code" != 0
      then
        exit 0
      fi
      sleep 0.1
    done
  '';
in
{
  process.manager.implementation = "process-compose";
  packages = [ pkgs.jq ];

  # TTY processes retain process-compose's PTY through the task wrapper.
  processes.tty-watch = {
    exec = ''
      test -t 0 || {
        touch "$DEVENV_STATE/tty-watch-no-tty"
        exit 70
      }
      touch "$DEVENV_STATE/tty-watch-ready"
      if ! IFS= read -r _; then
        touch "$DEVENV_STATE/tty-watch-stdin-eof"
        exit 71
      fi
    '';
    process-compose.is_tty = true;
    restart.on = "never";
  };

  # Interactive processes also retain task dependencies instead of bypassing
  # devenv-tasks. process-compose still owns their terminal interaction.
  processes.repl = {
    exec = "sleep infinity";
    process-compose.is_interactive = true;
  };

  # A regular process must still be routed through `devenv-tasks` so that
  # task-dependency handling and env injection keep working.
  processes.web = {
    exec = "sleep infinity";
  };

  processes.completed = {
    exec = "exit 0";
    restart.on = "never";
  };

  processes.failed = {
    exec = "exit 7";
    restart.on = "never";
  };

  assertions = [
    {
      assertion = lib.hasInfix "devenv-tasks" pcProcesses.repl.command;
      message = "interactive process should route through devenv-tasks. Got: ${pcProcesses.repl.command}";
    }
    {
      assertion = lib.hasInfix "--supervisor=external" pcProcesses.repl.command;
      message = "interactive process should delegate supervision to process-compose. Got: ${pcProcesses.repl.command}";
    }
    {
      assertion = lib.hasInfix "devenv-tasks" pcProcesses.tty-watch.command;
      message = "TTY process should route through devenv-tasks. Got: ${pcProcesses.tty-watch.command}";
    }
    {
      assertion = lib.hasInfix "devenv-tasks" pcProcesses.web.command;
      message = "non-interactive process should still route through devenv-tasks. Got: ${pcProcesses.web.command}";
    }
  ];

  enterTest = ''
    timeout 15 bash -c 'until test -e "$DEVENV_STATE/tty-watch-ready"; do sleep 0.1; done'
    test ! -e "$DEVENV_STATE/tty-watch-no-tty"
    test ! -e "$DEVENV_STATE/tty-watch-stdin-eof"

    timeout 15 ${waitForSettledProcesses}

    # Give a closed stdin enough time to terminate the child and expose the
    # old false-Running wrapper behaviour.
    sleep 0.5
    test ! -e "$DEVENV_STATE/tty-watch-stdin-eof"
    echo "external supervision preserves PTYs and propagates process exits"
  '';
}
