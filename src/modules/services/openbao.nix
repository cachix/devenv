{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.openbao;

  settingsFormat = pkgs.formats.json { };

  port = config.processes.openbao.ports.http.value;
  clusterPort = config.processes.openbao.ports.cluster.value;

  # Wildcard addresses are fine to bind to, but not to connect to.
  connectHost =
    if
      builtins.elem cfg.host [
        "0.0.0.0"
        "::"
        "[::]"
      ]
    then
      "127.0.0.1"
    else
      cfg.host;

  baseUrl = "http://${connectHost}:${toString port}";

  # One directory per backend: switching backends starts over
  # instead of pointing a backend at data it cannot read.
  # A path set through `settings` takes precedence, so setup manages the directory OpenBao uses.
  dataDir = lib.attrByPath [
    "storage"
    cfg.storage.backend
    "path"
  ] "${cfg.stateDir}/${cfg.storage.backend}"
    cfg.settings;
  initHashFile = "${dataDir}.init.sha256";

  # OpenBao 2.7 replaced the file backend with PebbleDB.
  hasPebbleDB = lib.versionAtLeast cfg.package.version "2.7";

  # The process reads the token from its environment during self-initialization,
  # so it does not end up in the generated configuration file.
  tokenEnvVar = "DEVENV_OPENBAO_TOKEN";

  # Labeled HCL blocks, as JSON: a list keeps them in order.
  toInitialize = block: {
    ${block.name}.request = map
      (request: {
        ${request.name} = removeAttrs request [ "name" ];
      })
      block.requests;
  };

  initialize = map toInitialize (
    [
      {
        name = "devenv-token";
        requests = [
          {
            name = "create-token";
            operation = "update";
            path = "auth/token/create";
            data = {
              id = {
                eval_source = "env";
                eval_type = "string";
                env_var = tokenEnvVar;
                require_present = true;
              };
              policies = [ "root" ];
              # The root token used during self-initialization is revoked afterwards,
              # which would also revoke tokens created as its children.
              no_parent = true;
              display_name = "devenv";
            };
          }
        ];
      }
    ]
    ++ lib.optional cfg.kv.enable {
      name = "devenv-kv";
      requests = [
        {
          name = "mount-kv";
          operation = "update";
          path = "sys/mounts/${cfg.kv.path}";
          data = {
            type = "kv";
            options.version = "2";
          };
        }
      ];
    }
    ++ cfg.initialize
  );

  # Self-initialization only runs against empty storage.
  # Track what it ran with, to warn when the configuration changes afterwards.
  initHash = builtins.hashString "sha256" (
    builtins.toJSON {
      inherit initialize;
      inherit (cfg) token;
    }
  );

  generatedSettings = {
    ui = cfg.ui.enable;

    storage.${cfg.storage.backend} = {
      path = dataDir;
    }
    // lib.optionalAttrs (cfg.storage.backend == "raft") { node_id = "devenv"; };

    # A single block as an object rather than a list,
    # so that `settings.listener.tcp` merges into it instead of replacing it.
    listener.tcp = {
      address = "${cfg.host}:${toString port}";
      cluster_address = "${cfg.host}:${toString clusterPort}";
      tls_disable = true;
    };

    seal.static = {
      current_key_id = "devenv";
      current_key = "file://${cfg.unsealKeyFile}";
    };

    api_addr = baseUrl;
    cluster_addr = "https://${connectHost}:${toString clusterPort}";
  };

  configFile = settingsFormat.generate "openbao.json" (
    lib.recursiveUpdate generatedSettings cfg.settings // { inherit initialize; }
  );

  requestType = lib.types.submodule {
    freeformType = lib.types.attrsOf settingsFormat.type;

    options = {
      name = lib.mkOption {
        type = lib.types.strMatching "[A-Za-z_][A-Za-z0-9_-]*";
        description = "The name of the request, unique within its block.";
      };

      operation = lib.mkOption {
        type = lib.types.str;
        example = "update";
        description = "The operation of the request, such as `read`, `update` or `delete`.";
      };

      path = lib.mkOption {
        type = lib.types.str;
        example = "sys/auth/userpass";
        description = "The API path of the request, without the `/v1/` prefix.";
      };
    };
  };

  initializeType = lib.types.submodule {
    options = {
      name = lib.mkOption {
        type = lib.types.strMatching "[A-Za-z_][A-Za-z0-9_-]*";
        description = "The name of the block.";
      };

      requests = lib.mkOption {
        type = lib.types.listOf requestType;
        description = ''
          The requests of the block, run in order.
          Besides `name`, `operation` and `path`, requests accept `data`, `token`
          and `allow_failure`, as documented by OpenBao.
        '';
      };
    };
  };
in
{
  options.services.openbao = {
    enable = lib.mkEnableOption "OpenBao server for secrets management";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of OpenBao to use.";
      default = pkgs.openbao;
      defaultText = lib.literalExpression "pkgs.openbao";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = ''
        The host address the OpenBao server listens on.
        Wrap IPv6 addresses in brackets.
      '';
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8200;
      description = ''
        The base port the OpenBao API listens on.
        The actual port is allocated by devenv and may differ;
        `BAO_ADDR` points to it.
      '';
    };

    clusterPort = lib.mkOption {
      type = lib.types.port;
      default = 8201;
      description = ''
        The base port the OpenBao cluster listener listens on.
        OpenBao always starts it; the actual port is allocated by devenv and may differ.
      '';
    };

    storage.backend = lib.mkOption {
      type = lib.types.enum [
        "pebbledb"
        "raft"
        "file"
      ];
      default = if hasPebbleDB then "pebbledb" else "file";
      defaultText = lib.literalMD ''`"pebbledb"` for OpenBao 2.7 and later, `"file"` before'';
      description = ''
        The storage backend OpenBao keeps its data in, in a directory under `stateDir`.

        `pebbledb` (OpenBao 2.7 and later) and `file` (before 2.7) start instantly.
        `raft` works with every version, but takes several seconds
        to elect itself leader on the first start.
      '';
    };

    stateDir = lib.mkOption {
      type = lib.types.str;
      default = "${config.devenv.state}/openbao";
      defaultText = lib.literalExpression ''"''${config.devenv.state}/openbao"'';
      description = ''
        The directory OpenBao stores its data and unseal key in.

        Self-initialization only runs against empty storage: remove it to start over.
      '';
    };

    token = lib.mkOption {
      type = lib.types.str;
      default = "root";
      description = ''
        The ID of a token with the root policy, created during self-initialization.
        `BAO_TOKEN` is set to it.

        Changing it requires removing `stateDir`, because self-initialization
        only runs on the first start.
      '';
    };

    unsealKeyFile = lib.mkOption {
      type = lib.types.str;
      default = "${cfg.stateDir}/unseal.key";
      defaultText = lib.literalExpression ''"''${config.services.openbao.stateDir}/unseal.key"'';
      description = ''
        The 32-byte key the static seal uses to unseal OpenBao on startup.
        A random key is generated if the file does not exist.
      '';
    };

    ui.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to serve the web UI at `/ui`.";
    };

    kv = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether to mount a KV version 2 secrets engine during self-initialization.";
      };

      path = lib.mkOption {
        type = lib.types.str;
        default = "secret";
        description = "The path of the KV secrets engine.";
      };
    };

    initialize = lib.mkOption {
      type = lib.types.listOf initializeType;
      default = [ ];
      example = lib.literalExpression ''
        [
          {
            name = "userpass";
            requests = [
              {
                name = "enable-userpass";
                operation = "update";
                path = "sys/auth/userpass";
                data.type = "userpass";
              }
              {
                name = "add-admin";
                operation = "update";
                path = "auth/userpass/users/admin";
                data = {
                  password = "admin";
                  token_policies = [ "root" ];
                };
              }
            ];
          }
        ]
      '';
      description = ''
        Additional self-initialization blocks, run in order after the built-in ones
        on the first start.
        See <https://openbao.org/docs/configuration/self-init/>.

        Changes only apply after removing `stateDir`.
      '';
    };

    settings = lib.mkOption {
      type = settingsFormat.type;
      default = { };
      example = {
        log_level = "debug";
        listener.tcp.max_request_duration = "30s";
      };
      description = ''
        OpenBao server configuration, merged recursively over the generated configuration,
        such as `listener.tcp` and `storage.<backend>`.
        See <https://openbao.org/docs/configuration/> for available settings.

        Use `initialize` for self-initialization instead of `settings.initialize`.
      '';
    };

    environmentVariables = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Additional environment variables for the OpenBao server process.
        Self-initialization requests can read them with the `env` source.
      '';
    };

    exportVaultEnv = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to also export `VAULT_ADDR` and `VAULT_TOKEN`,
        for clients that only support Vault, such as Terraform.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.storage.backend != "pebbledb" || hasPebbleDB;
        message = ''services.openbao.storage.backend = "pebbledb" requires OpenBao 2.7 or later.'';
      }
      {
        assertion = cfg.storage.backend != "file" || !hasPebbleDB;
        message = ''services.openbao.storage.backend = "file" was removed in OpenBao 2.7; use "pebbledb".'';
      }
    ];

    packages = [ cfg.package ];

    # Defaults, so that clients can be pointed elsewhere, such as at the devenv proxy.
    # The Vault variables follow overrides of the OpenBao ones.
    env = lib.mapAttrs (_: lib.mkDefault) (
      {
        BAO_ADDR = baseUrl;
        BAO_TOKEN = cfg.token;
      }
      // lib.optionalAttrs cfg.exportVaultEnv {
        VAULT_ADDR = config.env.BAO_ADDR;
        VAULT_TOKEN = config.env.BAO_TOKEN;
      }
    );

    processes.openbao = {
      exec = "exec ${lib.getExe cfg.package} server -config=${configFile}";

      env = cfg.environmentVariables // {
        ${tokenEnvVar} = cfg.token;
      };

      ports.http.allocate = cfg.port;
      ports.cluster.allocate = cfg.clusterPort;

      # Healthy only once OpenBao is initialized, unsealed and active.
      ready.http.get = {
        host = connectHost;
        inherit port;
        path = "/v1/sys/health";
      };
    };

    tasks."devenv:openbao:setup" = {
      exec = ''
        set -euo pipefail

        data_dir=${lib.escapeShellArg dataDir}
        key_file=${lib.escapeShellArg cfg.unsealKeyFile}
        hash_file=${lib.escapeShellArg initHashFile}

        mkdir -p "$data_dir"

        if [ ! -e "$key_file" ]; then
          mkdir -p "$(dirname "$key_file")"
          (umask 077 && head -c 32 /dev/urandom > "$key_file")
        fi

        if [ -z "$(ls -A "$data_dir")" ]; then
          # Empty storage: self-initialization runs with the current configuration.
          echo ${initHash} > "$hash_file"
        elif [ -e "$hash_file" ] && [ "$(cat "$hash_file")" != ${initHash} ]; then
          echo "devenv: services.openbao.token, kv or initialize changed after OpenBao was initialized." >&2
          echo "devenv: Self-initialization only runs on the first start; remove $data_dir to apply the changes." >&2
        fi
      '';
      before = [ "devenv:processes:openbao" ];
    };
  };
}
