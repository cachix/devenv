{ pkgs, lib, config, ... }:

let
  cfg = config.services.kafka;

  stateDir = config.devenv.state + "/kafka";

  # Port allocation helpers
  # Parse listener string like "PLAINTEXT://localhost:9092" to extract port
  parseListenerPort = listener:
    let
      # Remove protocol prefix (e.g., "PLAINTEXT://")
      withoutProtocol = lib.last (lib.splitString "://" listener);
      # Get the port part after ":"
      parts = lib.splitString ":" withoutProtocol;
    in
    lib.toInt (lib.last parts);

  # Parse listener to get host
  parseListenerHost = listener:
    let
      withoutProtocol = lib.last (lib.splitString "://" listener);
      parts = lib.splitString ":" withoutProtocol;
    in
    lib.head parts;

  # Parse listener to get protocol
  parseListenerProtocol = listener:
    lib.head (lib.splitString "://" listener);

  # Rebuild listener with new port
  rebuildListener = listener: newPort:
    let
      protocol = parseListenerProtocol listener;
      host = parseListenerHost listener;
    in
    "${protocol}://${host}:${toString newPort}";

  # Find first PLAINTEXT listener port (for main port)
  findPlaintextPort = listeners:
    let
      plaintext = lib.filter (l: lib.hasPrefix "PLAINTEXT://" l) listeners;
    in
    if plaintext == [ ] then 9092 else parseListenerPort (lib.head plaintext);

  # Find CONTROLLER listener port
  findControllerPort = listeners:
    let
      controller = lib.filter (l: lib.hasPrefix "CONTROLLER://" l) listeners;
    in
    if controller == [ ] then 9093 else parseListenerPort (lib.head controller);

  # Base ports from configuration
  basePort = findPlaintextPort cfg.settings."listeners";
  baseControllerPort = findControllerPort cfg.settings."listeners";

  # Allocated ports
  allocatedPort = config.processes.kafka.ports.main.value;
  allocatedControllerPort = config.processes.kafka.ports.controller.value;

  # Rebuild listeners with allocated ports
  rebuildListeners = listeners:
    map
      (l:
        if lib.hasPrefix "PLAINTEXT://" l then rebuildListener l allocatedPort
        else if lib.hasPrefix "CONTROLLER://" l then rebuildListener l allocatedControllerPort
        else l
      )
      listeners;

  # Parse controller.quorum.voters like "1@localhost:9093" and rebuild with allocated port
  rebuildQuorumVoters = voters:
    let
      parts = lib.splitString "@" voters;
      id = lib.head parts;
      hostPort = lib.last parts;
      hostParts = lib.splitString ":" hostPort;
      host = lib.head hostParts;
    in
    "${id}@${host}:${toString allocatedControllerPort}";

  mkPropertyString =
    let
      render = {
        bool = lib.boolToString;
        int = toString;
        list = lib.concatMapStringsSep "," mkPropertyString;
        string = lib.id;
      };
    in
    v: render.${builtins.typeOf v} v;

  stringlySettings = lib.mapAttrs (_: mkPropertyString)
    (lib.filterAttrs (_: v: v != null) cfg.settings);

  generator = (pkgs.formats.javaProperties { }).generate;
in
{
  options.services.kafka = {
    enable = lib.mkEnableOption "Apache Kafka";

    defaultMode = lib.mkOption {
      description = ''
        Which defaults to set for the mode Kafka should run in
        - `kraft` (default): Run Kafka in KRaft mode, Which requires no extra configuration.
        - `zookeeper`: Run Kafka in Zookeeper mode, this requires more configuration.
      '';
      default = "kraft";
      type = lib.types.enum [ "zookeeper" "kraft" ];
    };

    settings = lib.mkOption {
      description = ''
        [Kafka broker configuration](https://kafka.apache.org/documentation.html#brokerconfigs)
        {file}`server.properties`.

        Note that .properties files contain mappings from string to string.
        Keys with dots are NOT represented by nested attrs in these settings,
        but instead as quoted strings (ie. `settings."broker.id"`, NOT
        `settings.broker.id`).
      '';
      default = { };
      type = lib.types.submodule {
        freeformType = with lib.types; let
          primitive = oneOf [ bool int str ];
        in
        lazyAttrsOf (nullOr (either primitive (listOf primitive)));

        options = {
          "broker.id" = lib.mkOption {
            description = "Broker ID. -1 or null to auto-allocate in zookeeper mode.";
            default = null;
            type = with lib.types; nullOr int;
          };

          "log.dirs" = lib.mkOption {
            description = "Log file directories.";
            # Deliberaly leave out old default and use the rewrite opportunity
            # to have users choose a safer value -- /tmp might be volatile and is a
            # slightly scary default choice.
            default = [ "${stateDir}/logs" ];
            defaultText = lib.literalExpression ''[ "''${config.devenv.state + "/kafka"}/logs" ]'';
            type = with lib.types; listOf path;
          };

          "listeners" = lib.mkOption {
            description = ''
              Kafka Listener List.
              See [listeners](https://kafka.apache.org/documentation/#brokerconfigs_listeners).
              If you change this, you should also update the readiness probe.
            '';
            type = lib.types.listOf lib.types.str;
            default = [ "PLAINTEXT://localhost:9092" ];
          };
        };
      };
    };

    configFiles.serverProperties = lib.mkOption {
      description = ''
        Kafka server.properties configuration file path.
        Defaults to the rendered `settings`.
      '';
      type = lib.types.path;
    };

    configFiles.log4jProperties = lib.mkOption {
      description = "Kafka log4j property configuration file path";
      type = lib.types.path;
      default = pkgs.writeText "log4j.properties" cfg.log4jProperties;
      defaultText = ''pkgs.writeText "log4j.properties" cfg.log4jProperties'';
    };

    formatLogDirs = lib.mkOption {
      description = ''
        Whether to format log dirs in KRaft mode if all log dirs are
        unformatted, ie. they contain no meta.properties.
      '';
      type = lib.types.bool;
      default = true;
    };

    formatLogDirsIgnoreFormatted = lib.mkOption {
      description = ''
        Whether to ignore already formatted log dirs when formatting log dirs,
        instead of failing. Useful when replacing or adding disks.
      '';
      type = lib.types.bool;
      default = true;
    };

    log4jProperties = lib.mkOption {
      description = "Kafka log4j property configuration.";
      default = ''
        log4j.rootLogger=INFO, stdout

        log4j.appender.stdout=org.apache.log4j.ConsoleAppender
        log4j.appender.stdout.layout=org.apache.log4j.PatternLayout
        log4j.appender.stdout.layout.ConversionPattern=[%d] %p %m (%c)%n
      '';
      type = lib.types.lines;
    };

    jvmOptions = lib.mkOption {
      description = "Extra command line options for the JVM running Kafka.";
      default = [ ];
      type = lib.types.listOf lib.types.str;
      example = [
        "-Djava.net.preferIPv4Stack=true"
        "-Dcom.sun.management.jmxremote"
        "-Dcom.sun.management.jmxremote.local.only=true"
      ];
    };

    package = lib.mkPackageOption pkgs "apacheKafka" { };

    jre = lib.mkOption {
      description = "The JRE with which to run Kafka";
      default = cfg.package.passthru.jre;
      defaultText = lib.literalExpression "pkgs.apacheKafka.passthru.jre";
      type = lib.types.package;
    };
  };

  config =
    let
      # From config file example
      clusterIdFile = stateDir + "/clusterid";

      getOrGenerateClusterId = ''
        CLUSTER_ID=$(cat ${clusterIdFile} 2>/dev/null || ${cfg.package}/bin/kafka-storage.sh random-uuid | tee ${clusterIdFile})
      '';

      formatLogDirsScript = pkgs.writeShellScriptBin "format-log-dirs"
        (if cfg.formatLogDirsIgnoreFormatted then ''
          ${getOrGenerateClusterId}
          ${cfg.package}/bin/kafka-storage.sh format -t "$CLUSTER_ID" -c ${cfg.configFiles.serverProperties} --ignore-formatted
        '' else ''
          if ${lib.concatMapStringsSep " && " (l: ''[ ! -f "${l}/meta.properties" ]'') cfg.settings."log.dirs"}; then
            ${getOrGenerateClusterId}
            ${cfg.package}/bin/kafka-storage.sh format -t "$CLUSTER_ID" -c ${cfg.configFiles.serverProperties}
          fi
        '');

      startKafka = pkgs.writeShellScriptBin "start-kafka" ''
        set -e

        mkdir -p ${stateDir}
        ${formatLogDirsScript}/bin/format-log-dirs

        exec ${cfg.jre}/bin/java \
          -cp "${cfg.package}/libs/*" \
          -Dlog4j.configuration=file:${cfg.configFiles.log4jProperties} \
          ${toString cfg.jvmOptions} \
          kafka.Kafka \
          ${cfg.configFiles.serverProperties}
      '';
    in
    lib.mkMerge [
      (lib.mkIf (cfg.defaultMode == "kraft") {
        services.kafka.settings = {
          "process.roles" = lib.mkDefault [ "broker" "controller" ];
          "broker.id" = lib.mkDefault 1;
          "controller.quorum.voters" = lib.mkDefault "1@localhost:9093";
          "listeners" = lib.mkDefault [ "PLAINTEXT://localhost:9092" "CONTROLLER://localhost:9093" ];
          "inter.broker.listener.name" = lib.mkDefault "PLAINTEXT";
          "advertised.listeners" = lib.mkDefault [ "PLAINTEXT://localhost:9092" ];
          "controller.listener.names" = lib.mkDefault [ "CONTROLLER" ];
          "listener.security.protocol.map" = lib.mkDefault [
            "CONTROLLER:PLAINTEXT"
            "PLAINTEXT:PLAINTEXT"
            "SSL:SSL"
            "SASL_PLAINTEXT:SASL_PLAINTEXT"
            "SASL_SSL:SASL_SSL"
          ];

          "num.network.threads" = lib.mkDefault 3;
          "num.io.threads" = lib.mkDefault 8;
          "socket.send.buffer.bytes" = lib.mkDefault 102400;
          "socket.receive.buffer.bytes" = lib.mkDefault 102400;
          "socket.request.max.bytes" = lib.mkDefault 104857600;

          "num.partitions" = lib.mkDefault 1;
          "num.recovery.threads.per.data.dir" = lib.mkDefault 1;
          "offsets.topic.replication.factor" = lib.mkDefault 1;
          "transaction.state.log.replication.factor" = lib.mkDefault 1;
          "transaction.state.log.min.isr" = lib.mkDefault 1;

          "log.retention.hours" = lib.mkDefault 168;
          "log.segment.bytes" = lib.mkDefault 1073741824;
          "log.retention.check.interval.ms" = lib.mkDefault 300000;
        };
      })
      (lib.mkIf cfg.enable {
        packages = [ cfg.package ];
        env.KAFKA_PORT = allocatedPort;
        env.KAFKA_CONTROLLER_PORT = allocatedControllerPort;

        # Overlay allocated ports at config file generation to avoid infinite
        # recursion (#2494).  Writing back into cfg.settings would create a
        # cycle because basePort/baseControllerPort read from the same attrs.
        services.kafka.configFiles.serverProperties =
          let
            portOverrides = {
              "listeners" = mkPropertyString (rebuildListeners cfg.settings."listeners");
            } // lib.optionalAttrs (cfg.settings ? "advertised.listeners") {
              "advertised.listeners" = mkPropertyString (rebuildListeners cfg.settings."advertised.listeners");
            } // lib.optionalAttrs (cfg.settings ? "controller.quorum.voters") {
              "controller.quorum.voters" = mkPropertyString (rebuildQuorumVoters cfg.settings."controller.quorum.voters");
            };
          in
          generator "server.properties" (stringlySettings // portOverrides);

        processes.kafka = {
          ports.main.allocate = basePort;
          ports.controller.allocate = baseControllerPort;
          exec = "${startKafka}/bin/start-kafka";

          process-compose = {
            readiness_probe = {
              exec.command = "${cfg.package}/bin/kafka-topics.sh --list --bootstrap-server localhost:${toString allocatedPort}";
              initial_delay_seconds = 5;
              period_seconds = 10;
              timeout_seconds = 5;
              success_threshold = 1;
              failure_threshold = 3;
            };
          };
        };
      })
    ];
}
