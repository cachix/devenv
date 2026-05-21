{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.garage;
  types = lib.types;

  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  baseS3Port = parsePort cfg.s3Address;
  baseAdminPort = parsePort cfg.adminAddress;

  allocatedS3Port = config.processes.garage.ports.s3.value;
  allocatedAdminPort = config.processes.garage.ports.admin.value;
  allocatedRpcPort = config.processes.garage.ports.rpc.value;

  s3Host = parseHost cfg.s3Address;
  adminHost = parseHost cfg.adminAddress;

  configFile = pkgs.writeText "garage.toml" ''
    metadata_dir = "${config.env.DEVENV_STATE}/garage/meta"
    data_dir = "${config.env.DEVENV_STATE}/garage/data"
    db_engine = "lmdb"

    replication_factor = ${toString cfg.replicationFactor}

    rpc_bind_addr = "[::]:${toString allocatedRpcPort}"
    rpc_public_addr = "127.0.0.1:${toString allocatedRpcPort}"
    rpc_secret = "${cfg.rpcSecret}"

    [s3_api]
    s3_region = "${cfg.region}"
    api_bind_addr = "${s3Host}:${toString allocatedS3Port}"

    [admin]
    api_bind_addr = "${adminHost}:${toString allocatedAdminPort}"
    admin_token = "${cfg.adminToken}"

    ${cfg.extraConfig}
  '';

  garageFunctions = ''
    function garageHealthEndpoint() {
      ${lib.getExe pkgs.curl} \
        -f -H 'Authorization: Bearer ${cfg.adminToken}' \
        'http://${adminHost}:${toString allocatedAdminPort}/v1/health' 2>/dev/null
    }

    function garageHealthy() {
      if garageHealthEndpoint | ${lib.getExe pkgs.jq} -e '.status == "healthy"'; then
        return 0
      fi

      return 1
    }

    function garageRunning() {
      garageHealthEndpoint
    }
  '';

  configureScript =
    pkgs.writeShellScriptBin "configure"
      # bash
      ''
        set -euo pipefail
        ${garageFunctions}

        GARAGE="${cfg.package}/bin/garage -c ${configFile}"

        until garageRunning; do
          echo "Garage not ready, waiting..."
          sleep 1
        done

        echo "Garage ready."
        echo "Configuring layout ..."
        # Apply the cluster layout once. Garage rejects S3 traffic until at
        # least one node has a role, so this step is mandatory before the
        # buckets can be touched.
        if $GARAGE status 2>/dev/null | grep -q "NO ROLE ASSIGNED"; then
          NODE_ID=$($GARAGE node id 2>/dev/null | cut -d@ -f1)
          $GARAGE layout assign -z dc1 -c 1G "$NODE_ID"
          $GARAGE layout apply --version 1
        fi

        echo "Create all buckets ..."
        for bucket in ${lib.concatMapStringsSep " " (x: "'${x}'") cfg.buckets}; do
          if $GARAGE bucket info "$bucket" &>/dev/null; then
              echo "Bucket '$bucket' already exists, skipping"
          else
            echo "Create bucket '$bucket'."
            $GARAGE bucket create "$bucket"
          fi
        done

        ${cfg.afterStart}
      '';

  readyScript =
    pkgs.writeShellScriptBin "ready"
      # bash
      ''
        set -euo pipefail
        ${garageFunctions}

        GARAGE="${cfg.package}/bin/garage -c ${configFile}"

        garageHealthy || {
            echo "Garage not ready."
            exit 1
        }

        # Check that all buckets are created.
        for bucket in ${lib.concatMapStringsSep " " (x: "'${x}'") cfg.buckets}; do
          if $GARAGE bucket info "$bucket" &>/dev/null; then
            echo "Bucket '$bucket' exists."
          else
            echo "Bucket '$bucket' does not yet exist, waiting..."
            exit 1
          fi
        done

        echo "Garage ready and setup."
      '';

  startScript =
    pkgs.writeShellScriptBin "start"
      # bash
      ''
        echo "Setup folders."
        mkdir -p "${config.env.DEVENV_STATE}/garage/meta" \
          "${config.env.DEVENV_STATE}/garage/data"

        exec ${cfg.package}/bin/garage -c ${configFile} server
      '';
in
{
  options.services.garage = {
    enable = lib.mkEnableOption "Garage S3-compatible object storage";

    package = lib.mkOption {
      default = pkgs.garage_2;
      defaultText = lib.literalExpression "pkgs.garage_2";
      type = types.package;
      description = "Garage package to use.";
    };

    s3Address = lib.mkOption {
      default = "127.0.0.1:3900";
      type = types.str;
      description = "IP address and port of the S3 API.";
    };

    adminAddress = lib.mkOption {
      default = "127.0.0.1:3903";
      type = types.str;
      description = "IP address and port of the admin API.";
    };

    region = lib.mkOption {
      default = "us-east-1";
      type = types.str;
      description = ''
        S3 region label reported by the server. Defaults to AWS's canonical
        `us-east-1`.
      '';
    };

    replicationFactor = lib.mkOption {
      default = 1;
      type = types.int;
      description = ''
        Cluster replication factor. Single-node devenv setups always use 1.
      '';
    };

    rpcSecret = lib.mkOption {
      default = "0000000000000000000000000000000000000000000000000000000000000000";
      type = types.str;
      description = ''
        RPC secret as 64 hex characters. Hard-coded for single-node dev;
        production deployments override this with a real secret.
      '';
    };

    adminToken = lib.mkOption {
      default = "devtoken";
      type = types.str;
      description = ''
        Admin API bearer token. Hard-coded for single-node dev; production
        deployments override this with a real secret.
      '';
    };

    buckets = lib.mkOption {
      default = [ ];
      type = types.listOf types.str;
      description = "List of buckets to ensure exist on startup.";
    };

    afterStart = lib.mkOption {
      type = types.lines;
      default = "";
      example = ''
        garage key new --name app-key
        garage bucket allow --read --write --owner my-bucket --key app-key
      '';
      description = ''
        Bash code to execute after the server is running and the cluster
        layout is applied. The `garage` CLI in scope already points at the
        local instance via the generated config.
      '';
    };

    extraConfig = lib.mkOption {
      type = types.lines;
      default = "";
      description = ''
        Additional `garage.toml` snippet appended to the generated config.
      '';
    };

    ui = {
      enable = lib.mkEnableOption "Enable a simple web UI.";

      start = lib.mkOption {
        type = types.bool;
        default = false;
        description = ''
          If the service is by default started or must be manually started.
        '';
      };

      port = lib.mkOption {
        type = types.port;
        default = 3919;
        example = 3919;
        description = ''
          On which port the UI should run.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.adminToken != "";
        message = "services.garage.adminToken is empty; the admin API ready probe will fail silently.";
      }
      {
        assertion = cfg.rpcSecret != "";
        message = "services.garage.rpcSecret is empty.";
      }
    ];

    packages = [ cfg.package ];

    processes.garage = {
      ports.s3.allocate = baseS3Port;
      ports.admin.allocate = baseAdminPort;
      ports.rpc.allocate = 3901;
      exec = "exec ${startScript}/bin/start";
      ready.exec = "exec ${readyScript}/bin/ready";
    };

    env = {
      GARAGE_S3_PORT = toString allocatedS3Port;
      GARAGE_ADMIN_PORT = toString allocatedAdminPort;
      GARAGE_S3_ENDPOINT = "http://${s3Host}:${toString allocatedS3Port}";
      GARAGE_CONFIG_FILE = "${configFile}";
    };

    processes.garage-configure = {
      exec = "exec ${configureScript}/bin/configure";
      after = [ "devenv:processes:garage@started" ];
    };

    processes.garage-web-ui = lib.mkIf cfg.ui.enable {
      exec = "${lib.getExe pkgs.garage-webui}";
      start.enable = cfg.ui.start;
      env = {
        CONFIG_PATH = "${configFile}";
        PORT = lib.toString cfg.ui.port;
      };
      after = [ "devenv:processes:garage@ready" ];
    };
  };
}
