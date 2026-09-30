{ pkgs, config, lib, ... }:

let
  cfg = config.services.opentelemetry-collector;
  types = lib.types;

  settingsFormat = pkgs.formats.yaml { };

  defaultSettings = {
    extensions = {
      health_check = {
        endpoint = "localhost:13133";
      };
    };
    service = {
      extensions = [ "health_check" ];
    };
  };

  mergedSettings = lib.recursiveUpdate defaultSettings cfg.settings;

  # Keep allocation inputs separate from the generated configuration: feeding
  # resolved ports back into cfg.settings would make evaluation recursive.
  asAttrs = value: if builtins.isAttrs value then value else { };
  parseEndpoint = endpoint:
    if builtins.isString endpoint && !(lib.hasInfix "\${" endpoint)
    then builtins.match "(.*):([0-9]+)" endpoint
    else null;
  endpointListener = name: path: settings: defaultEndpoint:
    let
      endpoint = settings.endpoint or defaultEndpoint;
      parts = parseEndpoint endpoint;
    in
    lib.optional (parts != null && (settings.transport or "tcp") == "tcp" && lib.toInt (builtins.elemAt parts 1) != 0) {
      inherit name;
      path = path ++ [ "endpoint" ];
      host = builtins.head parts;
      basePort = lib.toInt (builtins.elemAt parts 1);
      kind = "endpoint";
    };

  healthSettings = asAttrs (mergedSettings.extensions.health_check or { });
  healthEnabled = builtins.elem "health_check" (mergedSettings.service.extensions or [ ]);
  healthListeners = lib.optionals healthEnabled (
    endpointListener "health" [ "extensions" "health_check" ] healthSettings "localhost:13133"
  );

  receiverListeners = lib.concatMap
    (receiverName:
      let
        receiver = asAttrs mergedSettings.receivers.${receiverName};
        protocols = asAttrs (receiver.protocols or { });
        portName = protocol:
          if receiverName == "otlp" then protocol
          else "otlp-${lib.removePrefix "otlp/" receiverName}-${protocol}";
      in
      lib.concatMap
        (protocol:
          lib.optionals (builtins.hasAttr protocol protocols) (
            endpointListener (portName protocol) [ "receivers" receiverName "protocols" protocol ]
              (asAttrs protocols.${protocol})
              (if protocol == "grpc" then "localhost:4317" else "localhost:4318")
          )
        ) [ "grpc" "http" ]
    )
    (builtins.filter (name: name == "otlp" || lib.hasPrefix "otlp/" name)
      (builtins.attrNames (mergedSettings.receivers or { })));

  metricsSettings = asAttrs (mergedSettings.service.telemetry.metrics or { });
  metricsEnabled = lib.toLower (metricsSettings.level or "normal") != "none";
  defaultReaders = [{
    pull.exporter.prometheus = {
      host = "localhost";
      port = 8888;
      without_scope_info = true;
      without_type_suffix = true;
      without_units = true;
    };
  }];
  readers = if (metricsSettings.readers or null) == null then defaultReaders else metricsSettings.readers;
  prometheusReaders = lib.filter (reader: reader.prometheus != null) (
    lib.imap0
      (index: reader: {
        inherit index;
        prometheus = reader.pull.exporter.prometheus or null;
      })
      readers
  );
  metricsListeners = lib.optionals metricsEnabled (lib.imap0
    (index: reader: {
      name = if index == 0 then "metrics" else "metrics-${toString reader.index}";
      basePort = reader.prometheus.port or 8888;
      kind = "port";
      readerIndex = reader.index;
    })
    (lib.filter
      (reader: builtins.isInt (reader.prometheus.port or 8888)
        && (reader.prometheus.port or 8888) != 0)
      prometheusReaders));

  listeners = healthListeners ++ receiverListeners ++ metricsListeners;
  allocatedPort = listener: config.processes.opentelemetry-collector.ports.${listener.name}.value;
  allocatedReaders = lib.imap0
    (index: reader:
      lib.foldl'
        (settings: listener:
          if listener.readerIndex == index
          then lib.recursiveUpdate settings { pull.exporter.prometheus.port = allocatedPort listener; }
          else settings
        )
        reader
        metricsListeners
    )
    readers;
  settingsWithMetrics = lib.recursiveUpdate mergedSettings (lib.optionalAttrs metricsEnabled {
    service.telemetry.metrics.readers = allocatedReaders;
  });
  finalSettings = lib.foldl'
    (settings: listener:
      if listener.kind == "endpoint"
      then lib.recursiveUpdate settings (lib.setAttrByPath listener.path "${listener.host}:${toString (allocatedPort listener)}")
      else settings
    )
    settingsWithMetrics
    listeners;

  healthHost = if healthListeners == [ ] then "localhost" else (builtins.head healthListeners).host;
  probeHost =
    if healthHost == "" || healthHost == "0.0.0.0" then "127.0.0.1"
    else if healthHost == "[::]" then "[::1]"
    else healthHost;

  otelConfig =
    if cfg.configFile == null
    then settingsFormat.generate "otel-config.yaml" finalSettings
    else cfg.configFile;
in
{
  options.services.opentelemetry-collector = {
    enable = lib.mkEnableOption "opentelemetry-collector";

    package = lib.mkOption {
      type = types.package;
      description = "The OpenTelemetry Collector package to use";
      default = pkgs.opentelemetry-collector-contrib;
      defaultText = lib.literalExpression "pkgs.opentelemetry-collector-contrib";
    };

    configFile = lib.mkOption {
      type = types.nullOr types.path;
      description = ''
        Override the configuration file used by OpenTelemetry Collector.
        By default, a configuration is generated from `services.opentelemetry-collector.settings`.

        Raw configuration files are not rewritten and their ports are not automatically allocated.
        Declare process ports and reference their resolved values when generating a raw file,
        and override the readiness probe to match its health endpoint.

        If overriding, enable the `health_check` extension to allow the readiness probe to check whether the Collector is ready.
        Otherwise, disable the readiness probe by setting `processes.opentelemetry-collector.ready = lib.mkForce null;`.
      '';
      default = null;
      example = lib.literalExpression ''
        pkgs.writeTextFile { name = "otel-config.yaml"; text = "..."; }
      '';
    };

    settings = lib.mkOption {
      type = types.submodule { freeformType = settingsFormat.type; };
      description = ''
        OpenTelemetry Collector configuration.
        Literal TCP ports in the health_check extension, OTLP receivers, and internal
        Prometheus metrics readers are used as allocation bases. The generated configuration
        uses the resolved values from `processes.opentelemetry-collector.ports`.
        Refer to https://opentelemetry.io/docs/collector/configuration/
        for more information on how to configure the Collector.
      '';
      default = defaultSettings;
      defaultText = lib.literalExpression "defaultSettings";
    };
  };

  config = lib.mkIf cfg.enable {
    changelogs = [
      {
        date = "2026-09-30";
        title = "services.opentelemetry-collector: allocate listener ports dynamically";
        when = cfg.enable && cfg.configFile == null;
        description = ''
          Settings-generated configurations now allocate free ports for the health check,
          internal Prometheus metrics, and configured OTLP gRPC/HTTP listeners. This prevents
          port collisions when running collectors in multiple devenv projects.

          Configured ports are allocation bases rather than fixed listening ports. Update
          clients and scrapers to use `config.processes.opentelemetry-collector.ports.<name>.value`,
          where the standard port names are `health`, `metrics`, `grpc`, and `http`.

          Supplied `services.opentelemetry-collector.configFile` overrides remain untouched
          and their ports are not automatically allocated.
        '';
      }
    ];

    processes.opentelemetry-collector = {
      ports = lib.mkIf (cfg.configFile == null) (builtins.listToAttrs (map
        (listener: {
          inherit (listener) name;
          value.allocate = listener.basePort;
        })
        listeners));
      exec = "exec ${lib.getExe cfg.package} --config ${lib.escapeShellArg otelConfig}";

      # A literal port 0 requests an ephemeral listener, so there is no
      # allocated port for devenv's HTTP readiness probe to target.
      ready = if cfg.configFile == null && healthListeners == [ ] then null else {
        http.get = {
          host = if cfg.configFile == null then probeHost else "localhost";
          scheme = "http";
          path = if cfg.configFile == null then healthSettings.path or "/" else "/";
          port = if cfg.configFile == null then config.processes.opentelemetry-collector.ports.health.value else 13133;
        };
        initial_delay = 2;
        probe_timeout = 5;
      };
    };
  };
}
