{ pkgs, lib, config, ... }:
let
  pcProcesses = config.process.managers.process-compose.settings.processes;
  foo = pcProcesses.foo;
  bar = pcProcesses.bar;
in
{
  process.manager.implementation = "process-compose";

  # Process with derived `ready` + `restart`, plus user overrides at leaf level
  # under `process-compose.*`. The bug being tested: shallow `//` used to drop
  # `exec.command` (from `ready.exec`) and the derived `restart` value when the
  # user set any sibling under `readiness_probe` / `availability`.
  processes.foo = {
    # Disabled: we only care about eval-time merge correctness, not runtime.
    start.enable = false;
    exec = "sleep infinity";

    ready = {
      exec = "true";
      failure_threshold = 5;
    };

    restart.on = "on_failure";
    shutdown = {
      signal = 2;
      grace = 3;
    };

    process-compose = {
      readiness_probe.failure_threshold = 99;
      availability.max_restarts = 7;
    };
  };

  # devenv-tasks receives SIGTERM and translates it for the service.
  processes.bar = {
    exec = "sleep infinity";
    shutdown = {
      signal = 2;
      grace = 1;
    };
  };

  assertions = [
    {
      assertion = (foo.readiness_probe.exec.command or null) == "true";
      message = "process-compose merge: readiness_probe.exec.command lost from `ready.exec`. Got: ${builtins.toJSON (foo.readiness_probe or {})}";
    }
    {
      assertion = foo.readiness_probe.failure_threshold == 99;
      message = "process-compose merge: user override of readiness_probe.failure_threshold not applied. Got: ${toString (foo.readiness_probe.failure_threshold or null)}";
    }
    {
      assertion = foo.availability.restart == "on_failure";
      message = "process-compose merge: availability.restart lost when user set availability.max_restarts. Got: ${toString (foo.availability.restart or null)}";
    }
    {
      assertion = foo.availability.max_restarts == 7;
      message = "process-compose merge: user override of availability.max_restarts not applied. Got: ${toString (foo.availability.max_restarts or null)}";
    }
    {
      assertion = foo.shutdown.signal == 2 && foo.shutdown.timeout_seconds == 11;
      message = "process-compose merge: shutdown settings of a direct process were not translated. Got: ${builtins.toJSON (foo.shutdown or {})}";
    }
    {
      assertion = bar.shutdown.signal == 15 && bar.shutdown.timeout_seconds == 7;
      message = "process-compose merge: a devenv-tasks wrapped process must receive SIGTERM. Got: ${builtins.toJSON (bar.shutdown or {})}";
    }
  ];

  enterTest = ''
    echo "process-compose merge assertions passed"
  '';
}
