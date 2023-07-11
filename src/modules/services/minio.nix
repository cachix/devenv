{ pkgs, lib, config, ... }:

let
  cfg = config.services.minio;
  types = lib.types;
  json = pkgs.formats.json { };

  serverCommand = lib.escapeShellArgs [
    "${cfg.package}/bin/minio"
    "server"
    "--json"
    "--address"
    cfg.listenAddress
    "--console-address"
    cfg.consoleAddress
    "--config-dir=${config.env.MINIO_CONFIG_DIR}"
    config.env.MINIO_DATA_DIR
  ];

  startScript = ''
    mkdir -p "$MINIO_DATA_DIR" "$MINIO_CONFIG_DIR"
    for bucket in ${lib.escapeShellArgs cfg.buckets}; do
      mkdir -p "$MINIO_DATA_DIR/$bucket"
    done
  '' + (if cfg.afterStart != "" then ''
    ${serverCommand} &

    while ! mc admin info local >& /dev/null; do
      sleep 1
    done

    ${cfg.afterStart}

    wait
  '' else ''
    exec ${serverCommand}
  '');

  clientWrapper = pkgs.writeShellScriptBin "mc" ''
    mkdir -p "$MINIO_CLIENT_CONFIG_DIR"
    install -m 0644 \
      '${json.generate "mc-config.json" cfg.clientConfig}' \
      "$MINIO_CLIENT_CONFIG_DIR/config.json"
    exec ${cfg.clientPackage}/bin/mc --config-dir "$MINIO_CLIENT_CONFIG_DIR" "$@"
  '';
in
{

  options.services.minio = {
    enable = lib.mkEnableOption "MinIO Object Storage";

    listenAddress = lib.mkOption {
      default = "127.0.0.1:9000";
      type = types.str;
      description = "IP address and port of the server.";
    };

    consoleAddress = lib.mkOption {
      default = "127.0.0.1:9001";
      type = types.str;
      description = "IP address and port of the web UI (console).";
    };

    accessKey = lib.mkOption {
      default = "minioadmin";
      type = types.str;
      description = ''
        Access key of 5 to 20 characters in length that clients use to access the server.
      '';
    };

    secretKey = lib.mkOption {
      default = "minioadmin";
      type = types.str;
      description = ''
        Specify the Secret key of 8 to 40 characters in length that clients use to access the server.
      '';
    };

    region = lib.mkOption {
      default = "us-east-1";
      type = types.str;
      description = ''
        The physical location of the server. By default it is set to us-east-1, which is same as AWS S3's and MinIO's default region.
      '';
    };

    browser = lib.mkOption {
      default = true;
      type = types.bool;
      description = "Enable or disable access to web UI.";
    };

    package = lib.mkOption {
      default = pkgs.minio;
      defaultText = lib.literalExpression "pkgs.minio";
      type = types.package;
      description = "MinIO package to use.";
    };

    buckets = lib.mkOption {
      default = [ ];
      type = types.listOf types.str;
      description = ''
        List of buckets to ensure exist on startup.
      '';
    };

    clientPackage = lib.mkOption {
      default = pkgs.minio-client;
      defaultText = lib.literalExpression "pkgs.minio-client";
      type = types.package;
      description = "MinIO client package to use.";
    };

    clientConfig = lib.mkOption {
      type = types.nullOr json.type;
      description = ''
        Contents of the mc `config.json`, as a nix attribute set.

        By default, `local` is configured to connect to the devenv minio service.
        Use `lib.mkForce null` to use your regular mc configuration from `$HOME/.mc` instead.
      '';
    };

    afterStart = lib.mkOption {
      type = types.lines;
      description = "Bash code to execute after minio is running.";
      default = "";
      example = ''
        mc anonymous set download local/mybucket
      '';
    };
  };

  config = lib.mkIf cfg.enable {

    assertions = [
      {
        assertion = cfg.afterStart != "" -> lib.hasAttrByPath [ "aliases" "local" ] cfg.clientConfig;
        message = "minio 'afterStart' script requires a 'local' alias in client config";
      }
    ];

    processes.minio.exec = "${startScript}";

    env.MINIO_DATA_DIR = config.env.DEVENV_STATE + "/minio/data";
    env.MINIO_CONFIG_DIR = config.env.DEVENV_STATE + "/minio/config";
    env.MINIO_REGION = "${cfg.region}";
    env.MINIO_BROWSER = "${if cfg.browser then "on" else "off"}";
    env.MINIO_ROOT_USER = "${cfg.accessKey}";
    env.MINIO_ROOT_PASSWORD = "${cfg.secretKey}";
    env.MINIO_CLIENT_CONFIG_DIR = config.env.DEVENV_STATE + "/minio/mc";

    packages = [
      (if cfg.clientConfig == null then cfg.clientPackage else clientWrapper)
    ];

    services.minio.clientConfig = lib.mkBefore {
      version = "10";
      aliases.local = {
        url = "http://${if lib.hasPrefix ":" cfg.listenAddress then "localhost:${cfg.listenAddress}" else cfg.listenAddress}";
        inherit (cfg) accessKey secretKey;
        api = "S3v4";
        path = "auto";
      };
    };

  };
}
