{ pkgs, lib, config, ... }:
let
  pcProcesses = config.process.managers.process-compose.settings.processes;
  foo = pcProcesses.foo;
  bar = pcProcesses.bar;
  nativeWatch = pcProcesses.nativeWatch;
  nativePort = pcProcesses.nativePort;
in
{
  process.manager.implementation = "process-compose";

  # User leaves must override derived process-compose policy without replacing siblings.
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

  # File-watch policy remains under native supervision.
  processes.nativeWatch = {
    exec = "sleep infinity";
    restart.on = "always";
    watch.paths = [ ./devenv.nix ];
  };

  # Implicit TCP readiness remains under native supervision.
  processes.nativePort = {
    exec = "sleep infinity";
    ports.http.allocate = 18080;
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
    {
      assertion = foo.shutdown.signal == 2 && foo.shutdown.timeout_seconds == 8;
      message = "process-compose merge: shutdown settings of a direct process were not translated. Got: ${builtins.toJSON (foo.shutdown or {})}";
    }
    {
      assertion = bar.shutdown.signal == 15 && bar.shutdown.timeout_seconds == 6;
      message = "process-compose merge: a devenv-tasks wrapped process must receive SIGTERM. Got: ${builtins.toJSON (bar.shutdown or {})}";
    }
    {
      assertion = config.processes.bar.supervisionMode == "external"
        && lib.hasInfix "external" config.process.taskCommandsBase.bar;
      message = "process-compose merge: fully translated policy should use external supervision. Mode: ${config.processes.bar.supervisionMode}; command: ${config.process.taskCommandsBase.bar}";
    }
    {
      assertion = config.processes.nativeWatch.supervisionMode == "native"
        && nativeWatch.availability.restart == "no"
        && lib.hasInfix "native" config.process.taskCommandsBase.nativeWatch;
      message = "process-compose merge: unsupported watch policy must remain under native supervision. Mode: ${config.processes.nativeWatch.supervisionMode}; command: ${config.process.taskCommandsBase.nativeWatch}";
    }
    {
      assertion = config.processes.nativePort.supervisionMode == "native"
        && nativePort.availability.restart == "no";
      message = "process-compose merge: implicit TCP readiness must remain under native supervision";
    }
  ];

  enterTest = ''
    echo "process-compose merge assertions passed"
  '';
}
