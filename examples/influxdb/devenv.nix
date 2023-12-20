{ pkgs, config, ... }:

let
  cfg = config.services.influxdb;
in
{
  packages = [
    pkgs.influxdb
  ];

  services.influxdb.enable = true;
  services.influxdb.config = ''
    [meta]
      dir = "/tmp/influxdb/meta"

    [data]
      dir = "/tmp/influxdb/data"
      wal-dir = "/tmp/influxdb/wal"
      query-log-enabled = true
      cache-max-memory-size = 1048576000
      cache-snapshot-memory-size = 26214400
      cache-snapshot-write-cold-duration = "10m"
      compact-full-write-cold-duration = "4h"

    [coordinator]
      write-timeout = "10s"
      max-concurrent-queries = 0
      query-timeout = "0s"
      log-queries-after = "0s"
      max-select-point = 0
      max-select-series = 0
      max-select-buckets = 0

    [retention]
      enabled = true
      check-interval = "30m"

    [shard-precreation]
      enabled = true
      check-interval = "10m"
      advance-period = "30m"

    [monitor]
      store-enabled = true
      store-database = "_internal"
      store-interval = "10s"

    [http]
      enabled = true
      bind-address = ":8087"
      auth-enabled = false
      log-enabled = true
      write-tracing = false
      pprof-enabled = true
      https-enabled = false

    [logging]
      format = "auto"
      level = "info"
      suppress-logo = false

    [[graphite]]
      enabled = false

    [[collectd]]
      enabled = false

    [[opentsdb]]
      enabled = false

    [[udp]]
      enabled = false
  '';
}
