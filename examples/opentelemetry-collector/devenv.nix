{ config, ... }:

let
  clickhouseEndpoint = "tcp://127.0.0.1:${toString config.services.clickhouse.port}?dial_timeout=10s&compress=lz4";
  dbName = "otel";
in
{
  services.clickhouse = {
    enable = true;
    port = 9000;
  };

  tasks."app:create-database" = {
    description = "Create the ClickHouse database before launching OpenTelemetry Collector";
    exec = ''
      clickhouse client "CREATE DATABASE IF NOT EXISTS ${dbName}"
    '';
    before = [ "devenv:processes:opentelemetry-collector" ];
  };

  processes.opentelemetry-collector.after = [ "devenv:processes:clickhouse-server" ];

  services.opentelemetry-collector = {
    enable = true;

    # Or use a raw YAML file:
    # `services.opentelemetry-collector.configFile = pkgs.writeTextFile "otel-config.yaml" "...";`
    settings = {
      receivers = {
        otlp = {
          protocols = {
            grpc.endpoint = "localhost:4317";
            http.endpoint = "localhost:4318";
          };
        };
      };

      processors = {
        batch = {
          timeout = "5s";
          send_batch_size = 100000;
        };
      };

      exporters = {
        clickhouse = {
          endpoint = clickhouseEndpoint;
          database = dbName;
          ttl = "72h";
          logs_table_name = "otel_logs";
          traces_table_name = "otel_traces";
          metrics_table_name = "otel_metrics";
          timeout = "5s";
          retry_on_failure = {
            enabled = true;
            initial_interval = "5s";
            max_interval = "30s";
            max_elapsed_time = "300s";
          };
        };
      };

      service = {
        pipelines = {
          traces = {
            receivers = [ "otlp" ];
            processors = [ "batch" ];
            exporters = [ "clickhouse" ];
          };
        };
      };
    };
  };
}
