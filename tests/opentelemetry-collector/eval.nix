{
  lib,
  pkgs,
  module ? ../../src/modules/services/opentelemetry-collector.nix,
}:
let
  # Test module wiring with a deterministic allocator and inspect the generated
  # configuration without building YAML or binding network ports.
  format = pkgs.formats.yaml { };
  fakePkgs = pkgs // {
    formats = pkgs.formats // {
      yaml =
        _:
        format
        // {
          generate = name: settings: builtins.toFile name (builtins.toJSON settings);
        };
    };
  };
  evaluate =
    service:
    let
      evaluated = lib.evalModules {
        specialArgs.pkgs = fakePkgs;
        modules = [
          module
          (builtins.dirOf module + "/../changelogs.nix")
          {
            options.processes = lib.mkOption {
              type = lib.types.attrsOf (
                lib.types.submodule {
                  options = {
                    exec = lib.mkOption { type = lib.types.str; };
                    ready = lib.mkOption {
                      type = lib.types.nullOr (import (builtins.dirOf module + "/../lib/ready.nix") { inherit lib; });
                      default = null;
                    };
                    ports = lib.mkOption {
                      default = { };
                      type = lib.types.attrsOf (
                        lib.types.submodule (
                          { config, ... }: {
                            options = {
                              allocate = lib.mkOption { type = lib.types.port; };
                              value = lib.mkOption {
                                type = lib.types.port;
                                default = config.allocate + 100;
                              };
                            };
                          }
                        )
                      );
                    };
                  };
                }
              );
              default = { };
            };
            config.services.opentelemetry-collector = {
              enable = true;
            }
            // service;
          }
        ];
      };
      process = evaluated.config.processes.opentelemetry-collector;
      file = lib.removeSuffix "'" (
        lib.removePrefix "'" (lib.last (lib.splitString " --config " process.exec))
      );
    in
    {
      inherit process file;
      settings = builtins.fromJSON (builtins.readFile (builtins.unsafeDiscardStringContext file));
      ports = lib.mapAttrs (_: port: port.allocate) process.ports;
      changelogs = lib.filter (entry: entry.when) evaluated.config.changelogs;
    };
  standard = evaluate {
    settings.receivers.otlp.protocols = {
      grpc = { };
      http = { };
    };
  };
  custom = evaluate {
    settings = {
      extensions.health_check = {
        endpoint = "0.0.0.0:14000";
        path = "/health";
      };
      receivers."otlp/app".protocols = {
        grpc = {
          endpoint = "[::1]:15000";
          max_recv_msg_size_mib = 16;
        };
        http = null;
      };
      service.telemetry.metrics.readers = [
        { periodic.exporter.otlp.endpoint = "https://remote:4318"; }
        {
          pull.exporter.prometheus = {
            host = "::1";
            port = 16000;
            without_units = false;
          };
        }
        {
          pull.exporter.prometheus = {
            host = "localhost";
            port = 17000;
          };
        }
      ];
    };
  };
  disabled = evaluate {
    settings = {
      service.extensions = [ ];
      service.telemetry.metrics = {
        level = "none";
        readers = [ ];
      };
      receivers.otlp.protocols = {
        grpc = { };
        http = null;
      };
    };
  };
  unmanaged = evaluate {
    settings = {
      receivers.otlp.protocols = {
        grpc = {
          endpoint = "/tmp/otel.sock";
          transport = "unix";
        };
        http.endpoint = "\${env:OTEL_HTTP_ENDPOINT}";
      };
      service.telemetry.metrics.readers = [ { pull.exporter.prometheus.port = 0; } ];
    };
  };
  ipv6 = evaluate { settings.extensions.health_check.endpoint = "[::]:14100"; };
  rawFile = builtins.toFile "raw-otel-config.json" ''{"raw":true}'';
  raw = evaluate { configFile = rawFile; };
  defaults = evaluate { };
  serviceDisabled = evaluate { enable = false; };
  missingMetricsPort = evaluate {
    settings.service.telemetry.metrics.readers = [ { pull.exporter.prometheus.host = "127.0.0.1"; } ];
  };
  pushOnly = evaluate {
    settings.service.telemetry.metrics.readers = [
      { periodic.exporter.otlp.endpoint = "https://remote:4318"; }
    ];
  };
  templatedHealth = evaluate {
    settings.extensions.health_check.endpoint = "\${env:OTEL_HEALTH_ENDPOINT}";
  };
  ephemeralHealth = evaluate {
    settings.extensions.health_check.endpoint = "localhost:0";
  };
  nullProtocols = evaluate {
    settings.receivers.otlp.protocols = {
      grpc = null;
      http = null;
    };
  };
in
assert builtins.length standard.changelogs == 1;
assert raw.changelogs == [ ];
assert serviceDisabled.changelogs == [ ];
assert
  standard.ports == {
    health = 13133;
    metrics = 8888;
    grpc = 4317;
    http = 4318;
  };
assert standard.settings.extensions.health_check.endpoint == "localhost:13233";
assert standard.process.ready.http.get.port == 13233;
assert standard.settings.receivers.otlp.protocols.grpc.endpoint == "localhost:4417";
assert standard.settings.receivers.otlp.protocols.http.endpoint == "localhost:4418";
assert
  (builtins.head standard.settings.service.telemetry.metrics.readers).pull.exporter.prometheus == {
    host = "localhost";
    port = 8988;
    without_scope_info = true;
    without_type_suffix = true;
    without_units = true;
  };
assert
  custom.ports == {
    health = 14000;
    "otlp-app-grpc" = 15000;
    "otlp-app-http" = 4318;
    metrics = 16000;
    "metrics-2" = 17000;
  };
assert custom.process.ready.http.get.host == "127.0.0.1";
assert custom.process.ready.http.get.path == "/health";
assert custom.settings.receivers."otlp/app".protocols.grpc.endpoint == "[::1]:15100";
assert custom.settings.receivers."otlp/app".protocols.grpc.max_recv_msg_size_mib == 16;
assert custom.settings.receivers."otlp/app".protocols.http.endpoint == "localhost:4418";
assert
  (builtins.elemAt custom.settings.service.telemetry.metrics.readers 0)
  .periodic.exporter.otlp.endpoint == "https://remote:4318";
assert
  (builtins.elemAt custom.settings.service.telemetry.metrics.readers 1).pull.exporter.prometheus == {
    host = "::1";
    port = 16100;
    without_units = false;
  };
assert
  (builtins.elemAt custom.settings.service.telemetry.metrics.readers 2).pull.exporter.prometheus.port
  == 17100;
assert
  disabled.ports == {
    grpc = 4317;
    http = 4318;
  };
assert disabled.process.ready == null;
assert disabled.settings.service.telemetry.metrics.readers == [ ];
assert unmanaged.ports == { health = 13133; };
assert unmanaged.settings.receivers.otlp.protocols.grpc.endpoint == "/tmp/otel.sock";
assert unmanaged.settings.receivers.otlp.protocols.http.endpoint == "\${env:OTEL_HTTP_ENDPOINT}";
assert ipv6.process.ready.http.get.host == "::1";
assert ipv6.settings.extensions.health_check.endpoint == "[::]:14200";
assert raw.ports == { };
assert raw.file == rawFile;
assert raw.settings == { raw = true; };
assert raw.process.ready.http.get.port == 13133;
assert missingMetricsPort.ports.metrics == 8888;
assert
  (builtins.head missingMetricsPort.settings.service.telemetry.metrics.readers)
  .pull.exporter.prometheus == {
    host = "127.0.0.1";
    port = 8988;
  };
assert pushOnly.ports == { health = 13133; };
assert builtins.length pushOnly.settings.service.telemetry.metrics.readers == 1;
assert templatedHealth.ports == { metrics = 8888; };
assert templatedHealth.process.ready == null;
assert templatedHealth.settings.extensions.health_check.endpoint == "\${env:OTEL_HEALTH_ENDPOINT}";
assert !(builtins.hasAttr "health" ephemeralHealth.ports);
assert ephemeralHealth.process.ready == null;
assert ephemeralHealth.settings.extensions.health_check.endpoint == "localhost:0";
assert nullProtocols.ports.grpc == 4317;
assert nullProtocols.ports.http == 4318;
assert nullProtocols.settings.receivers.otlp.protocols.grpc.endpoint == "localhost:4417";
assert nullProtocols.settings.receivers.otlp.protocols.http.endpoint == "localhost:4418";
assert
  defaults.ports == {
    health = 13133;
    metrics = 8888;
  };
true
