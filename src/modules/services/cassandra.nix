{ pkgs, lib, config, ... }:

let
  cfg = config.services.cassandra;
  types = lib.types;
  baseDir = config.env.DEVENV_STATE + "/cassandra";
  JVM_OPTS =
    cfg.jvmOpts
    ++ lib.optionals (lib.versionAtLeast cfg.package.version "4") [
      "-Xlog:gc=warning,heap*=warning,age*=warning,safepoint=warning,promotion*=warning"
    ];
  cassandraConfig = lib.flip lib.recursiveUpdate cfg.extraConfig (
    {
      start_native_transport = cfg.allowClients;
      listen_address = cfg.listenAddress;
      commitlog_sync = "batch";
      commitlog_sync_batch_window_in_ms = 2;
      cluster_name = cfg.clusterName;
      partitioner = "org.apache.cassandra.dht.Murmur3Partitioner";
      endpoint_snitch = "SimpleSnitch";
      data_file_directories = [ "${baseDir}/data" ];
      commitlog_directory = "${baseDir}/commitlog";
      saved_caches_directory = "${baseDir}/saved_caches";
      hints_directory = "${baseDir}/hints";
      seed_provider = [
        {
          class_name = "org.apache.cassandra.locator.SimpleSeedProvider";
          parameters = [{ seeds = lib.concatStringsSep "," cfg.seedAddresses; }];
        }
      ];
    }
  );
  cassandraConfigFile = pkgs.writeText "cassandra.yaml" (builtins.toJSON cassandraConfig);
  startScript = pkgs.writeShellScriptBin "start-cassandra" ''
    set -euo pipefail

    if [[ ! -d "${baseDir}" ]]; then
      mkdir -p "${baseDir}"
    fi

    JVM_OPTS="${lib.concatStringsSep " " JVM_OPTS}" exec ${cfg.package}/bin/cassandra -Dcassandra.config=file:///${cassandraConfigFile} -f
  '';
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "cassandra" "enable" ] [
      "services"
      "cassandra"
      "enable"
    ])
  ];

  options.services.cassandra = {
    enable = lib.mkEnableOption "Add Cassandra process script.";

    package = lib.mkOption {
      type = types.package;
      description = "Which version of Cassandra to use";
      default = pkgs.cassandra_4;
      defaultText = lib.literalExpression "pkgs.cassandra_4";
      example = lib.literalExpression "pkgs.cassandra_4;";
    };

    listenAddress = lib.mkOption {
      type = types.str;
      description = "Listen address";
      default = "127.0.0.1";
      example = "127.0.0.1";
    };

    seedAddresses = lib.mkOption {
      type = types.listOf types.str;
      default = [ "127.0.0.1" ];
      description = "The addresses of hosts designated as contact points of the cluster";
    };

    clusterName = lib.mkOption {
      type = types.str;
      default = "Test Cluster";
      description = "The name of the cluster";
    };

    allowClients = lib.mkOption {
      type = types.bool;
      default = true;
      description = ''
        Enables or disables the native transport server (CQL binary protocol)
      '';
    };

    extraConfig = lib.mkOption {
      type = types.attrs;
      default = { };
      example =
        {
          commitlog_sync_batch_window_in_ms = 3;
        };
      description = ''
        Extra options to be merged into `cassandra.yaml` as nix attribute set.
      '';
    };

    jvmOpts = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Options to pass to the JVM through the JVM_OPTS environment variable";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package startScript ];

    processes.cassandra.exec = "${startScript}/bin/start-cassandra";
  };
}
