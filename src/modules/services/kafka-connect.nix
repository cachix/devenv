{ pkgs, lib, config, ... }:

let
  kafkaCfg = config.services.kafka;
  cfg = config.services.kafka.connect;
  types = lib.types;

  stateDir = config.env.DEVENV_STATE + "/kafka/connect";

  storageFile = stateDir + "/connect.offsets";

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

  stringlyGeneric = (attrs:
    lib.mapAttrs (_: mkPropertyString)
      (lib.filterAttrs (_: v: v != null) attrs)
  );

  stringlySettings = stringlyGeneric cfg.settings;

  generator = (pkgs.formats.javaProperties { }).generate;
in
{
  options.services.kafka.connect = {
    enable = lib.mkEnableOption "Kafka Connect";

    initialConnectors = lib.mkOption {
      type = types.listOf (types.submodule {
        freeformType = with lib.types; let
          primitive = oneOf [ bool int str ];
        in
        lazyAttrsOf (nullOr (either primitive (listOf primitive)));

        options = {
          name = lib.mkOption {
            type = types.str;
            description = ''
              Name of the connector
            '';
          };
          config = lib.mkOption {
            type = types.attrs;
            description = ''
              Initial configuration for the connector
            '';
          };
        };
      });
      default = [ ];
      description = ''
        List of Kafka Connect connectors to set up initially
      '';
    };

    settings = lib.mkOption {
      description = ''
        {file}`connect-standalone.properties`.

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
          "listeners" = lib.mkOption {
            type = types.nullOr (types.listOf types.str);
            default = null;
            description = ''
              List of listeners for Kafka Connect
              (By default Kafka Connect listens on http://localhost:8083)
            '';
            example = [ "http://localhost:8080" ];
          };

          "bootstrap.servers" = lib.mkOption {
            type = types.listOf types.str;
            description = ''
              A list of host/port pairs to use for establishing the initial connection to the Kafka cluster.
            '';
            default = [ "localhost:9092" ];
          };

          "plugin.path" = lib.mkOption {
            type = types.nullOr (types.listOf (types.either types.str types.path));
            default = null;
            description = ''
              The list should consist of top level directories that include any combination of:
              a) directories immediately containing jars with plugins and their dependencies
              b) uber-jars with plugins and their dependencies
              c) directories immediately containing the package directory structure of classes of plugins and their dependencies
              Note: symlinks will be followed to discover dependencies or plugins.
            '';
          };

          "offset.storage.file.filename" = lib.mkOption {
            type = types.str;
            default = storageFile;
            description = ''
              The file to store connector offsets in. By storing offsets on disk, a standalone process can be stopped and started on a single node and resume where it previously left off.
            '';
          };

          "offset.flush.interval.ms" = lib.mkOption {
            type = types.int;
            default = 10000;
            description = ''
              Interval at which to try committing offsets for tasks
            '';
          };

          "key.converter" = lib.mkOption {
            type = types.str;
            default = "org.apache.kafka.connect.json.JsonConverter";
            description = ''
              The key converter to use for the connector.
            '';
          };

          "value.converter" = lib.mkOption {
            type = types.str;
            default = "org.apache.kafka.connect.json.JsonConverter";
            description = ''
              The value converter to use for the connector.
            '';
          };

          "key.converter.schemas.enable" = lib.mkOption {
            type = types.bool;
            default = true;
            description = ''
              Whether the key converter should include schema information in the message.
            '';
          };

          "value.converter.schemas.enable" = lib.mkOption {
            type = types.bool;
            default = true;
            description = ''
              Whether the value converter should include schema information in the message.
            '';
          };
        };
      };
    };
  };

  config =
    let
      pkg = kafkaCfg.package;

      configFile = generator "connect-standalone.properties" stringlySettings;

      # TODO: make it work with .properties files?
      # connectorFiles = lib.lists.map (c: generator "connector-${c.name}.properties" (stringlyGeneric c)) cfg.initialConnectors;
      connectorFiles = lib.lists.map (c: pkgs.writeText "connector.json" (builtins.toJSON c)) cfg.initialConnectors;
      connectorFilesConcatted = lib.concatStringsSep " " connectorFiles;

      startKafkaConnect = pkgs.writeShellScriptBin "start-kafka-connect" ''
        mkdir -p ${stateDir}
        ${pkg}/bin/connect-standalone.sh ${configFile} ${connectorFilesConcatted}
      '';
    in
    (lib.mkIf cfg.enable (lib.mkIf kafkaCfg.enable {
      processes.kafka-connect = {
        exec = "${startKafkaConnect}/bin/start-kafka-connect";

        process-compose = {
          readiness_probe = {
            initial_delay_seconds = 2;
            http_get = {
              path = "/connectors";
              port = 8083;
            };
          };

          depends_on = {
            kafka = {
              condition = "process_healthy";
            };
          };
        };
      };

    }));
}
