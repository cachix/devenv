{ pkgs, ... }:
{
  packages = [
    pkgs.coreutils
    pkgs.python3
  ];
  enterShell = "echo DEVENV_SHELL_READY";
  tasks."sandbox:check".exec = ''
    printf task > "$DEVENV_ROOT/task-result"
    printf '{"sandbox": true}' > "$DEVENV_TASK_OUTPUT_FILE"
    if cat "$SANDBOX_OUTSIDE/secret"; then
      echo "Task read outside sandbox" >&2
      exit 1
    fi
    if touch "$SANDBOX_OUTSIDE/task-write"; then
      echo "Task wrote outside sandbox" >&2
      exit 1
    fi
  '';
  tasks."sandbox:status" = {
    status = ''
      if cat "$SANDBOX_OUTSIDE/secret"; then
        printf leaked > "$DEVENV_ROOT/status-leak"
      fi
      exit 1
    '';
    exec = "touch $DEVENV_ROOT/status-result";
  };
  tasks."sandbox:extra".exec = ''
    bash "$DEVENV_ROOT/extra-check.sh" task
  '';
  tasks."sandbox:network".exec = ''
    python3 "$DEVENV_ROOT/network-check.py" blocked
  '';
  tasks."sandbox:service" = {
    after = [ "devenv:processes:check" ];
    exec = "test -f $DEVENV_ROOT/process-result";
  };
  processes.check.ready = {
    exec = ''
      ! cat "$SANDBOX_OUTSIDE/secret" && test -f "$DEVENV_ROOT/process-result"
    '';
    period = 1;
  };
  processes.check.exec = ''
    if cat "$SANDBOX_OUTSIDE/secret"; then
      exit 1
    fi
    if touch "$SANDBOX_OUTSIDE/process-write"; then
      exit 1
    fi
    printf process > "$DEVENV_ROOT/process-result"
    exec sleep 3600
  '';
}
