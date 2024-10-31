{ pkgs, lib, config, ... }:

let
  kafkaCfg = config.services.kafka;
  cfg = config.services.kafka.connect;
  types = lib.types;

in
{
  options.services.kafka.connect = {
    enable = lib.mkEnableOption "Kafka Connect";
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
