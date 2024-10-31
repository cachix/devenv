{ pkgs, lib, config, ... }:

let
  kafkaCfg = config.services.kafka;
  cfg = config.services.kafka.connect;
  types = lib.types;

in
{
  options.services.kafka.connect = {
    enable = lib.mkEnableOption "Kafka Connect";

    plugins = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = ''
        List of Kafka Connect plugins to install
        The list should consist of top level directories that include any combination of:
        a) directories immediately containing jars with plugins and their dependencies
        b) uber-jars with plugins and their dependencies
        c) directories immediately containing the package directory structure of classes of plugins and their dependencies
        Note: symlinks will be followed to discover dependencies or plugins.
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
        ${lib.optionalString (lib.lists.length cfg.plugins <= 0) "plugin.path=${lib.concatStringsSep "," cfg.plugins}"}
      '';

      startKafkaConnect = pkgs.writeShellScriptBin "start-kafka-connect" ''
        mkdir -p ${stateDir}
        ${pkg}/bin/connect-standalone.sh ${configFile}
      '';
    in
    lib.mkIf cfg.enable (lib.mkIf kafkaCfg.enable {
      processes.kafka-connect.exec = "${startKafkaConnect}/bin/start-kafka-connect";
    });
}
