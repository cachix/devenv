{ ... }:

{
  services.clickhouse.enable = true;

  # Wait for clickhouse to come up
  processes.opentelemetry-collector.process-compose = {
    depends_on.clickhouse-server.condition = "process_healthy";
  };

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
          endpoint = "tcp://127.0.0.1:9000?dial_timeout=10s&compress=lz4";
          database = "otel";
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
