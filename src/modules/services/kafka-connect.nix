{ pkgs, lib, config, ... }:

let
  kafkaCfg = config.services.kafka;
  cfg = config.services.kafka.connect;
  types = lib.types;

in
{
  options.services.kafka.connect = {
    enable = lib.mkEnableOption "Kafka Connect";

    listeners = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = ''
        List of listeners for Kafka Connect
        (By default Kafka Connect listens on http://localhost:8083)
      '';
      example = [ "http://localhost:8080" ];
    };

    pluginDirectories = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = ''
        The list should consist of top level directories that include any combination of:
        a) directories immediately containing jars with plugins and their dependencies
        b) uber-jars with plugins and their dependencies
        c) directories immediately containing the package directory structure of classes of plugins and their dependencies
        Note: symlinks will be followed to discover dependencies or plugins.
      '';
    };

    initialConnectors = lib.mkOption {
      type = types.listOf (types.submodule {
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
  };

  config =
    let
      pkg = kafkaCfg.package;
      stateDir = config.env.DEVENV_STATE + "/kafka/connect";
      storageFile = stateDir + "/connect.offsets";

      configFile = pkgs.writeText "connect-standalone.properties" ''
        bootstrap.servers=localhost:9092
        key.converter=org.apache.kafka.connect.json.JsonConverter
        value.converter=org.apache.kafka.connect.json.JsonConverter
        key.converter.schemas.enable=true
        value.converter.schemas.enable=true
        offset.storage.file.filename=${storageFile}
        offset.flush.interval.ms=10000
        ${lib.optionalString (lib.lists.length cfg.listeners > 0) "listeners=${lib.concatStringsSep "," cfg.listeners}"}
        ${lib.optionalString (lib.lists.length cfg.pluginDirectories > 0) "plugin.path=${lib.concatStringsSep "," cfg.pluginDirectories}"}
      '';

      # Create a json file for each connector
      connectorFiles = lib.lists.map (c: pkgs.writeText "connector.json" (builtins.toJSON c)) cfg.initialConnectors;
      connectorFilesConcatted = lib.concatStringsSep " " connectorFiles;

      startKafkaConnect = pkgs.writeShellScriptBin "start-kafka-connect" ''
        mkdir -p ${stateDir}
        ${pkg}/bin/connect-standalone.sh ${configFile} ${connectorFilesConcatted}
      '';
    in
    lib.mkIf cfg.enable (lib.mkIf kafkaCfg.enable {
      processes.kafka-connect.exec = "${startKafkaConnect}/bin/start-kafka-connect";
    });
}
