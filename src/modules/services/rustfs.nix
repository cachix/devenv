{ pkgs, lib, config, ... }:

let
  cfg = config.services.rustfs;
  types = lib.types;

  rustfsInput = config.lib.getInput {
    name = "rustfs";
    url = "github:rustfs/rustfs/5d737eaeb7fcab5d40c655ba60a494e93dd98922";
    attribute = "services.rustfs.enable";
  };

  # Port allocation
  baseApiPort = cfg.port;
  baseConsolePort = cfg.consolePort;
  allocatedApiPort = config.processes.rustfs.ports.api.value;
  allocatedConsolePort = config.processes.rustfs.ports.console.value;

  apiAddr = "${cfg.bind}:${toString allocatedApiPort}";
  consoleAddr = "${cfg.bind}:${toString allocatedConsolePort}";
in
{
  options.services.rustfs = {
    enable = lib.mkEnableOption "RustFS object storage";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of RustFS to use";
      default = rustfsInput.packages.${pkgs.stdenv.system}.default;
      defaultText = lib.literalExpression "rustfsInput.packages.\${pkgs.stdenv.system}.default";
    };

    bind = lib.mkOption {
      type = types.nullOr types.str;
      default = "127.0.0.1";
      description = ''
        The IP interface to bind to.
        `null` means "all interfaces".
      '';
    };

    port = lib.mkOption {
      type = types.port;
      default = 9000;
      description = "The TCP port for the S3 API.";
    };

    consolePort = lib.mkOption {
      type = types.port;
      default = 9001;
      description = "The TCP port for the web console.";
    };

    consoleEnable = lib.mkOption {
      type = types.bool;
      default = true;
      description = "Enable or disable the web console.";
    };

    accessKey = lib.mkOption {
      type = types.str;
      default = "rustfsadmin";
      description = "Access key for authentication (5 to 20 characters).";
    };

    secretKey = lib.mkOption {
      type = types.str;
      default = "rustfsadmin";
      description = "Secret key for authentication (8 to 40 characters).";
    };

    extraEnvironment = lib.mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = ''
        Additional environment variables to pass to RustFS.
        See the RustFS documentation for available options
        (e.g. `RUSTFS_CORS_ALLOWED_ORIGINS`, `RUSTFS_TLS_PATH`).
      '';
      example = {
        RUSTFS_OBS_LOGGER_LEVEL = "debug";
        RUSTFS_OBJECT_CACHE_ENABLE = "true";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    processes.rustfs = {
      ports.api.allocate = baseApiPort;
      ports.console.allocate = baseConsolePort;
      exec = "exec ${cfg.package}/bin/rustfs \"$RUSTFS_DATA_DIR\"";

      ready.http.get = {
        host = cfg.bind;
        port = allocatedApiPort;
        path = "/health";
      };
    };

    env = {
      RUSTFS_PORT = allocatedApiPort;
      RUSTFS_CONSOLE_PORT = allocatedConsolePort;
      RUSTFS_ADDRESS = apiAddr;
      RUSTFS_CONSOLE_ADDRESS = consoleAddr;
      RUSTFS_CONSOLE_ENABLE = if cfg.consoleEnable then "true" else "false";
      RUSTFS_ACCESS_KEY = cfg.accessKey;
      RUSTFS_SECRET_KEY = cfg.secretKey;
      RUSTFS_DATA_DIR = config.env.DEVENV_STATE + "/rustfs/data";
    } // cfg.extraEnvironment;

    tasks."devenv:rustfs:setup" = {
      exec = ''
        mkdir -p "$RUSTFS_DATA_DIR"
      '';
      before = [ "devenv:processes:rustfs" ];
    };
  };
}
