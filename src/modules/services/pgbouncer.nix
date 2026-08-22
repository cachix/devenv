{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.pgbouncer;
  inherit (lib) types;

  basePort = cfg.port;
  allocatedPort = config.processes.pgbouncer.ports.main.value;

  parseKeyValueSections =
    section:
    let
      filterNulls = lib.filterAttrs (_: v: v != null);

      # { foo = { bar = "baz"; } } -> foo = bar=baz
      pairsToString = lib.mapAttrsToList (name: value: "${name}=${toString value}");
      genLine = pairs: lib.concatStringsSep " " (pairsToString (filterNulls pairs));

      resultSection = lib.mapAttrsToList (name: value: "${name} = ${genLine value}") section;
    in
    lib.concatStringsSep "\n" resultSection;

  settingsFormat = pkgs.formats.ini { };
  configFile =
    let
      # split cfg.settings by attrs and not attrs
      globalSection = lib.filterAttrs (_: v: !lib.isAttrs v) cfg.settings;
      otherSections = lib.filterAttrs (_: lib.isAttrs) cfg.settings;
      settings = otherSections // {
        pgbouncer = globalSection;
      };

      databasesSection = parseKeyValueSections cfg.databases;
      usersSection = parseKeyValueSections cfg.users;
      peersSection = parseKeyValueSections cfg.peers;
    in
    pkgs.runCommandLocal "pgbouncer.ini" { } (
      ''
        cat ${settingsFormat.generate "pgbouncer.ini" settings} >> $out

      ''
      + lib.optionalString (databasesSection != "") ''
        echo "[databases]" >> $out
        echo "${databasesSection}" >> $out
      ''
      + lib.optionalString (usersSection != "") ''
        echo "[users]" >> $out
        echo "${usersSection}" >> $out
      ''
      + lib.optionalString (peersSection != "") ''
        echo "[peers]" >> $out
        echo "${peersSection}" >> $out
      ''
    );
in
{
  options.services.pgbouncer = {
    enable = lib.mkEnableOption "pgbouncer";

    package = lib.mkOption {
      type = types.package;
      default = pkgs.pgbouncer;
      defaultText = lib.literalExpression "pkgs.pgbouncer";
    };

    port = lib.mkOption {
      type = types.port;
      default = 6432;
      description = ''
        The TCP port to accept connections.
        If port 0 is specified, PgBouncer will not listen on a TCP socket but
        a UNIX socket.
      '';
    };

    listen_addr = lib.mkOption {
      type = types.str;
      description = ''
        Specifies a list (comma-separated) of addresses where to listen for TCP
        connections. You may also use * meaning "listen on all addresses".

        When not set, only Unix socket connections are accepted.
      '';
      default = "";
      example = "127.0.0.1";
    };

    settings = lib.mkOption {
      type = types.attrsOf types.anything;
      default = { };
      description = ''
        PgBouncer configuration. Refer to <https://www.pgbouncer.org/config.html>
        for an overview of `pgbouncer.ini`.
      '';
      example = {
        pool_mode = "session";
        auth_type = "scram-sha-256";
        peer_id = 1;
      };
    };

    # these settings are separated from the option above, because pgbouncer
    # uses non-standard format specifically with these options. Example:
    # [databases]
    # foodb = host=host1.example.com port=5432

    databases = lib.mkOption {
      description = ''
        List of databases for PgBouncer to connect to.

        Aside from `dbname`, `host` and `port` you can also specify all other
        options from <https://www.pgbouncer.org/config.html#section-databases>.
      '';
      example = lib.literalExpression ''
        foodb = {
          host = "127.0.0.1";
          port = 5555;
        };
        bardb = {
          host = "localhost";
          # reroutes to foodb
          dbname = "foodb";
        };
      '';

      type = types.attrsOf (
        types.submodule {
          freeformType = types.attrsOf (types.either types.str types.int);
          options = {
            dbname = lib.mkOption {
              type = types.nullOr types.str;
              default = null;
              description = "Override for the destination database name.";
            };
            host = lib.mkOption {
              type = types.nullOr types.str;
              default = null;
              description = ''
                Host name or IP address to connect to.

                A comma-separated list of host names or addresses can be
                specified. In that case, connections are made in a round-robin
                manner.

                Defaults to a Unix socket.
              '';
            };
            port = lib.mkOption {
              type = types.int;
              default = 5432;
              description = "Port to connect to.";
            };
          };
        }
      );
    };

    users = lib.mkOption {
      type = types.attrsOf (types.attrsOf (types.either types.str types.int));
      description = ''
        List of settings overrides for specific users.

        See <https://www.pgbouncer.org/config.html#section-users>.
      '';
      example = {
        user1 = {
          pool_mode = "session";
          pool_size = 200;
        };
      };
    };

    peers = lib.mkOption {
      description = ''
        Peers that PgBouncer can forward cancellation requests to.

        See <https://www.pgbouncer.org/config.html#section-peers>.
      '';
      example = {
        "1" = {
          host = "host1.example.com";
        };
        "2" = {
          host = "/tmp/pgbouncer-2";
          port = 5555;
        };
      };

      type = types.attrsOf (
        types.submodule {
          freeformType = types.attrsOf (types.either types.str types.int);
          options = {
            host = lib.mkOption {
              type = types.str;
              description = "Host name or IP address to connect to.";
            };
            port = lib.mkOption {
              type = types.int;
              default = 6432;
              description = "Port to connect to.";
            };
            pool_size = lib.mkOption {
              type = types.nullOr types.int;
              default = null;
              description = ''
                Maximum number of cancel requests that can be in flight to the
                peer at the same time.

                If not set, the `default_pool_size` is used.
              '';
            };
          };
        }
      );
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion =
          let
            specialCases = [
              "databases"
              "users"
              "peers"
            ];
            isSpecial = x: lib.elem x specialCases;
            filteredOnlyAttrs = lib.filterAttrs (_: lib.isAttrs) cfg.settings;
          in
            !lib.any isSpecial (lib.attrNames filteredOnlyAttrs);

        message = ''
          You have specified `databases`, `users` or `peers` using
          `services.pgbouncer.settings`. Use specialized
          `services.pgbouncer.{databases,users,peers}` options instead
          that support pgbouncer's key=value syntax.
        '';
      }
    ];

    packages = [ cfg.package ];

    services.pgbouncer.settings = {
      inherit (cfg) listen_addr;
      listen_port = allocatedPort;
    };

    processes.pgbouncer = {
      ports.main.allocate = lib.mkIf (basePort != 0) basePort;
      exec = "exec ${lib.getExe cfg.package} ${configFile}";
    };
  };
}
