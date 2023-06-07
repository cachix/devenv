{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.elasticsearch;
  es7 = builtins.compareVersions cfg.package.version "7" >= 0;

  esConfig = ''
    network.host: ${cfg.listenAddress}
    cluster.name: ${cfg.cluster_name}
    ${lib.optionalString cfg.single_node "discovery.type: single-node"}
    http.port: ${toString cfg.port}
    transport.port: ${toString cfg.tcp_port}
    ${cfg.extraConf}
  '';

  elasticsearchYml = pkgs.writeTextFile {
    name = "elasticsearch.yml";
    text = esConfig;
  };

  loggingConfigFilename = "log4j2.properties";
  loggingConfigFile = pkgs.writeTextFile {
    name = loggingConfigFilename;
    text = cfg.logging;
  };

  esPlugins = pkgs.buildEnv {
    name = "elasticsearch-plugins";
    paths = cfg.plugins;
    postBuild = "${pkgs.coreutils}/bin/mkdir -p $out/plugins";
  };

  startScript = pkgs.writeShellScript "es-startup" ''
    set -e

    export ES_HOME="$ELASTICSEARCH_DATA"
    export ES_JAVA_OPTS="${toString cfg.extraJavaOptions}"
    export ES_PATH_CONF="$ELASTICSEARCH_DATA/config"
    mkdir -m 0700 -p "$ELASTICSEARCH_DATA"
    # Install plugins
    rm -f "$ELASTICSEARCH_DATA/plugins"
    ln -sf ${esPlugins}/plugins "$ELASTICSEARCH_DATA/plugins"
    rm -f "$ELASTICSEARCH_DATA/lib"
    ln -sf ${cfg.package}/lib "$ELASTICSEARCH_DATA/lib"
    rm -f "$ELASTICSEARCH_DATA/modules"
    ln -sf ${cfg.package}/modules "$ELASTICSEARCH_DATA/modules"

    # Create config dir
    mkdir -m 0700 -p "$ELASTICSEARCH_DATA/config"
    rm -f "$ELASTICSEARCH_DATA/config/elasticsearch.yml"
    cp ${elasticsearchYml} "$ELASTICSEARCH_DATA/config/elasticsearch.yml"
    rm -f "$ELASTICSEARCH_DATA/logging.yml"
    rm -f "$ELASTICSEARCH_DATA/config/${loggingConfigFilename}"
    cp ${loggingConfigFile} "$ELASTICSEARCH_DATA/config/${loggingConfigFilename}"

    mkdir -p "$ELASTICSEARCH_DATA/scripts"
    rm -f "$ELASTICSEARCH_DATA/config/jvm.options"

    cp ${cfg.package}/config/jvm.options "$ELASTICSEARCH_DATA/config/jvm.options"

    # Create log dir
    mkdir -m 0700 -p "$ELASTICSEARCH_DATA/logs"

    # Start it
    exec ${cfg.package}/bin/elasticsearch ${toString cfg.extraCmdLineOptions}
  '';

in
{
  imports = [
    (lib.mkRenamedOptionModule [ "elasticsearch" "enable" ] [ "services" "elasticsearch" "enable" ])
  ];

  options.services.elasticsearch = {
    enable = mkOption {
      description = "Whether to enable elasticsearch.";
      default = false;
      type = types.bool;
    };

    package = mkOption {
      description = "Elasticsearch package to use.";
      default = pkgs.elasticsearch7;
      defaultText = literalExpression "pkgs.elasticsearch7";
      type = types.package;
    };

    listenAddress = mkOption {
      description = "Elasticsearch listen address.";
      default = "127.0.0.1";
      type = types.str;
    };

    port = mkOption {
      description = "Elasticsearch port to listen for HTTP traffic.";
      default = 9200;
      type = types.int;
    };

    tcp_port = mkOption {
      description = "Elasticsearch port for the node to node communication.";
      default = 9300;
      type = types.int;
    };

    cluster_name = mkOption {
      description =
        "Elasticsearch name that identifies your cluster for auto-discovery.";
      default = "elasticsearch";
      type = types.str;
    };

    single_node = mkOption {
      description = "Start a single-node cluster";
      default = true;
      type = types.bool;
    };

    extraConf = mkOption {
      description = "Extra configuration for elasticsearch.";
      default = "";
      type = types.str;
      example = ''
        node.name: "elasticsearch"
        node.master: true
        node.data: false
      '';
    };

    logging = mkOption {
      description = "Elasticsearch logging configuration.";
      default = ''
        logger.action.name = org.elasticsearch.action
        logger.action.level = info
        appender.console.type = Console
        appender.console.name = console
        appender.console.layout.type = PatternLayout
        appender.console.layout.pattern = [%d{ISO8601}][%-5p][%-25c{1.}] %marker%m%n
        rootLogger.level = info
        rootLogger.appenderRef.console.ref = console
      '';
      type = types.str;
    };

    extraCmdLineOptions = mkOption {
      description =
        "Extra command line options for the elasticsearch launcher.";
      default = [ ];
      type = types.listOf types.str;
    };

    extraJavaOptions = mkOption {
      description = "Extra command line options for Java.";
      default = [ ];
      type = types.listOf types.str;
      example = [ "-Djava.net.preferIPv4Stack=true" ];
    };

    plugins = mkOption {
      description = "Extra elasticsearch plugins";
      default = [ ];
      type = types.listOf types.package;
      example =
        lib.literalExpression "[ pkgs.elasticsearchPlugins.discovery-ec2 ]";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = getName cfg.package != getName pkgs.opensearch;
        message = ''
          To use OpenSearch, you have to use the OpenSearch options. (services.opensearch.enable = true;)
        '';
      }
    ];

    env.ELASTICSEARCH_DATA = config.env.DEVENV_STATE + "/elasticsearch";

    processes.elasticsearch = {
      exec = "${startScript}";

      process-compose = {
        readiness_probe = {
          exec.command = "${pkgs.curl}/bin/curl -f -k http://${cfg.listenAddress}:${toString cfg.port}";
          initial_delay_seconds = 15;
          period_seconds = 10;
          timeout_seconds = 2;
          success_threshold = 1;
          failure_threshold = 5;
        };

        # https://github.com/F1bonacc1/process-compose#-auto-restart-if-not-healthy
        availability.restart = "on_failure";
      };
    };
  };
}
