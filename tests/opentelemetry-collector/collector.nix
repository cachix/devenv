{
  config,
  lib,
  pkgs,
  ...
}:
let
  process = config.processes.opentelemetry-collector;
  ports = lib.mapAttrs (_: port: port.value) process.ports;
in
{
  packages = [
    pkgs.curl
    pkgs.jq
    pkgs.grpcurl
  ];
  services.opentelemetry-collector = {
    enable = true;
    settings = {
      receivers.otlp.protocols = {
        grpc = { };
        http = { };
      };
      exporters.debug = { };
      service.pipelines.traces = {
        receivers = [ "otlp" ];
        exporters = [ "debug" ];
      };
    };
  };
  tasks."app:record-ports" = {
    before = [ "devenv:processes:opentelemetry-collector" ];
    exec = ''
      printf '%s\n' ${lib.escapeShellArg (builtins.toJSON ports)} > "$DEVENV_STATE/otel-ports.json"
    '';
  };
  scripts.validate-collector.exec = "${process.exec} validate";
  enterTest = ''
    export OTEL_PORTS_FILE="$DEVENV_STATE/otel-ports.json"
  '';
}
