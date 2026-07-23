{ pkgs, lib, config, ... }:
let
  pcProcesses = config.process.managers.process-compose.settings.processes;
  foo = pcProcesses.foo;
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
      probe_timeout = 4;
      failure_threshold = 5;
    };

    restart.on = "on_failure";

    process-compose = {
      readiness_probe.failure_threshold = 99;
      availability.max_restarts = 7;
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
      assertion = foo.readiness_probe.timeout_seconds == 4;
      message = "process-compose merge: ready.probe_timeout did not render to readiness_probe.timeout_seconds. Got: ${toString (foo.readiness_probe.timeout_seconds or null)}";
    }
    {
      assertion = foo.availability.restart == "on_failure";
      message = "process-compose merge: availability.restart lost when user set availability.max_restarts. Got: ${toString (foo.availability.restart or null)}";
    }
    {
      assertion = foo.availability.max_restarts == 7;
      message = "process-compose merge: user override of availability.max_restarts not applied. Got: ${toString (foo.availability.max_restarts or null)}";
    }
  ];

  enterTest = ''
    echo "process-compose merge assertions passed"
  '';
}
