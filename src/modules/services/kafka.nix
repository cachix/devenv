{ pkgs, lib, config, ... }:

let
  cfg = config.services.kafka;
  types = lib.types;

in
{
  options.services.kafka = {
    enable = lib.mkEnableOption "Apache Kafka";

    package = lib.mkOption {
      type = types.package;
      description = "Which Apache Kafka package to use";
      default = pkgs.apacheKafka;
      defaultText = "pkgs.apacheKafka";
    };

    # listenPort = lib.mkOption {
    #   description = "Kafka port to listen on.";
    #   default = 9092;
    #   type = types.port;
    # };

    # config = lib.mkOption {
    #   type = types.attrs;
    #   default = {};
    # };
  };

  config =
    let
      # From config file example
      stateDir = config.env.DEVENV_STATE + "/kafka";
      clusterIdFile = stateDir + "/clusterid";
      logsDir = stateDir + "/logs";
      # TODO: Make these options configurable
      serverProperties = pkgs.writeText "server.properties" ''
        process.roles=broker,controller
        node.id=1
        controller.quorum.voters=1@localhost:9093
        listeners=PLAINTEXT://:9092,CONTROLLER://:9093
        inter.broker.listener.name=PLAINTEXT
        advertised.listeners=PLAINTEXT://localhost:9092
        controller.listener.names=CONTROLLER
        listener.security.protocol.map=CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT,SSL:SSL,SASL_PLAINTEXT:SASL_PLAINTEXT,SASL_SSL:SASL_SSL
        num.network.threads=3
        num.io.threads=8
        socket.send.buffer.bytes=102400
        socket.receive.buffer.bytes=102400
        socket.request.max.bytes=104857600
        log.dir=${logsDir}
        num.partitions=1
        num.recovery.threads.per.data.dir=1
        offsets.topic.replication.factor=1
        transaction.state.log.replication.factor=1
        transaction.state.log.min.isr=1
        log.retention.hours=168
        log.segment.bytes=1073741824
        log.retention.check.interval.ms=300000
      '';

      startKafka = pkgs.writeShellScriptBin "start-kafka" ''
        set -e

        mkdir -p ${stateDir}
        CLUSTER_ID=$(cat ${clusterIdFile} 2>/dev/null || ${cfg.package}/bin/kafka-storage.sh random-uuid | tee ${clusterIdFile})
        # If logs dir is empty, format the storage
        if [ ! -d ${logsDir} ] || [ ! "$(ls -A ${logsDir})" ]; then
          ${cfg.package}/bin/kafka-storage.sh format -t $CLUSTER_ID -c ${serverProperties}
        fi
        ${cfg.package}/bin/kafka-server-start.sh ${serverProperties}
      '';
    in
    lib.mkIf cfg.enable {
      packages = [ cfg.package ];

      # processes.kafka-setup.exec = ''
      # '';
      processes.kafka.exec = "${startKafka}/bin/start-kafka";
    };
}
