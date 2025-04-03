{ config
, lib
, pkgs
, ...
}:

let
  cfg = config.services.keycloak;

  inherit (lib)
    mkIf
    mkMerge
    mkOption
    mkRenamedOptionModule
    mkPackageOption
    types
    ;

  inherit (types)
    nullOr
    oneOf
    listOf
    attrsOf
    ;

  assertStringPath =
    optionName: value:
    if builtins.isPath value then
      throw ''
        services.keycloak.${optionName}:
          ${builtins.toString value}
          is a Nix path, but should be a string, since Nix
          paths are copied into the world-readable Nix store.
      ''
    else
      value;
in
{
  imports = [
    (mkRenamedOptionModule [ "keycloak" "enable" ] [ "services" "keycloak" "enable" ])
  ];

  options.services.keycloak = {
    enable = mkOption {
      type = types.bool;
      default = false;
      example = true;
      description = ''
        Whether to enable the Keycloak identity and access management
        server.
      '';
    };

    sslCertificate = mkOption {
      type = nullOr types.path;
      default = null;
      example = "/run/keys/ssl_cert";
      apply = assertStringPath "sslCertificate";
      description = ''
        The path to a PEM formatted certificate to use for TLS/SSL
        connections.
        This file stays on your local disk and is not copied to the Nix store.
      '';
    };

    sslCertificateKey = mkOption {
      type = nullOr types.path;
      default = null;
      example = "/run/keys/ssl_key";
      apply = assertStringPath "sslCertificateKey";
      description = ''
        The path to a PEM formatted private key to use for TLS/SSL
        connections.
        This file stays on your local disk and is not copied to the Nix store.
      '';
    };

    plugins = mkOption {
      type = listOf types.path;
      default = [ ];
      description = ''
        Keycloak plugin jar, ear files or derivations containing
        them. Packaged plugins are available through
        `pkgs.keycloak.plugins`.
      '';
    };

    database = {
      type = mkOption {
        type = types.enum [
          "dev-mem"
        ];
        default = "dev-mem";
        example = "dev-mem";
        description = ''
          The type of database Keycloak should connect to.
          In a development setup is fine to just use 'dev-mem' which
          creates everything in memory.
        '';
      };
    };

    package = mkPackageOption pkgs "keycloak" { };

    initialAdminPassword = mkOption {
      type = types.str;
      default = "admin";
      description = ''
        Initial password set for the temporary `admin` user.
        The password is not stored safely and should be changed
        immediately in the admin panel.

        See [Admin bootstrap and recovery](https://www.keycloak.org/server/bootstrap-admin-recovery) for details.
      '';
    };

    realmFiles = mkOption {
      type = listOf types.path;
      example = lib.literalExpression ''
        [
          ./some/realm.json
          ./another/realm.json
        ]
      '';
      default = [ ];
      description = ''
        Realm files that the server is going to import during startup.
        If a realm already exists in the server, the import operation is
        skipped. Importing the master realm is not supported. All files are
        expected to be in `json` format. See the
        [documentation](https://www.keycloak.org/server/importExport) for
        further information.
      '';
    };

    settings = mkOption {
      type = lib.types.submodule {
        freeformType = attrsOf (
          nullOr (oneOf [
            types.str
            types.int
            types.bool
            (attrsOf types.path)
          ])
        );

        options = {
          http-enable = mkOption {
            type = types.bool;
            default = true;
            example = false;
            description = ''
              If the 'http' listener is enabled.
              In a dev. environment you normally dont care about HTTPS.
            '';
          };

          http-host = mkOption {
            type = types.str;
            default = "::";
            example = "::1";
            description = ''
              On which address Keycloak should accept new connections.
            '';
          };

          http-port = mkOption {
            type = types.port;
            default = 8080;
            example = 8080;
            description = ''
              On which port Keycloak should listen for new HTTP connections.
            '';
          };

          https-port = mkOption {
            type = types.port;
            default = 443;
            example = 8443;
            description = ''
              On which port Keycloak should listen for new HTTPS connections.
            '';
          };

          http-relative-path = mkOption {
            type = types.str;
            default = "/";
            example = "/auth";
            apply = x: if !(lib.hasPrefix "/") x then "/" + x else x;
            description = ''
              The path relative to `/` for serving
              resources.

              ::: {.note}
              In versions of Keycloak using Wildfly (&lt;17),
              this defaulted to `/auth`. If
              upgrading from the Wildfly version of Keycloak,
              i.e. a NixOS version before 22.05, you'll likely
              want to set this to `/auth` to
              keep compatibility with your clients.

              See <https://www.keycloak.org/migration/migrating-to-quarkus>
              for more information on migrating from Wildfly to Quarkus.
              :::
            '';
          };

          hostname = mkOption {
            type = types.str;
            default = "localhost";
            example = "localhost";
            description = ''
              The hostname part of the public URL used as base for
              all frontend requests.

              See <https://www.keycloak.org/server/hostname>
              for more information about hostname configuration.
            '';
          };

          hostname-backchannel-dynamic = mkOption {
            type = types.bool;
            default = false;
            example = true;
            description = ''
              Enables dynamic resolving of backchannel URLs,
              including hostname, scheme, port and context path.

              See <https://www.keycloak.org/server/hostname>
              for more information about hostname configuration.
            '';
          };
        };
      };

      example = lib.literalExpression ''
        {
          hostname = "localhost";
          https-key-store-file = "/path/to/file";
          https-key-store-password = { _secret = "/run/keys/store_password"; };
        }
      '';

      description = ''
        Configuration options corresponding to parameters set in
        {file}`conf/keycloak.conf`.

        Most available options are documented at <https://www.keycloak.org/server/all-config>.

        Options containing secret data should be set to an attribute
        set containing the attribute `_secret` - a
        string pointing to a file containing the value the option
        should be set to. See the example to get a better picture of
        this: in the resulting
        {file}`conf/keycloak.conf` file, the
        `https-key-store-password` key will be set
        to the contents of the
        {file}`/run/keys/store_password` file.
      '';
    };
  };

  config =
    let
      isSecret = v: lib.isAttrs v && v ? _secret && lib.isString v._secret;

      # Generate the keycloak config file to build it.
      keycloakConfig = lib.generators.toKeyValue {
        mkKeyValue = lib.flip lib.generators.mkKeyValueDefault "=" {
          mkValueString =
            v:
            if builtins.isInt v then
              toString v
            else if builtins.isString v then
              v
            else if true == v then
              "true"
            else if false == v then
              "false"
            else if isSecret v then
              builtins.hashString "sha256" v._secret
            else
              throw "unsupported type ${builtins.typeOf v}: ${(lib.generators.toPretty { }) v}";
        };
      };

      # Filters empty values out.
      filteredConfig = lib.converge
        (lib.filterAttrsRecursive (
          _: v:
            !builtins.elem v [
              { }
              null
            ]
        ))
        cfg.settings;

      # Write the keycloak config file.
      confFile = pkgs.writeText "keycloak.conf" (keycloakConfig filteredConfig);

      keycloakBuild = cfg.package.override {
        inherit confFile;
        plugins = cfg.package.enabledPlugins ++ cfg.plugins;
      };

    in
    mkIf cfg.enable {

      services.keycloak.settings = mkMerge [
        {
          db = cfg.database.type;
          health-enable = true;

          log-console-level = "debug";
          log-level = "debug";
        }
        (mkIf (cfg.sslCertificate != null && cfg.sslCertificateKey != null) {
          https-certificate-file = cfg.sslCertificate;
          https-certificate-key-file = cfg.sslCertificateKey;
        })
      ];

      packages = [ keycloakBuild ];

      env = {
        KC_HOME_DIR = config.env.DEVENV_STATE + "/keycloak";
        KC_CONF_DIR = config.env.DEVENV_STATE + "/keycloak/conf";
        KC_TMP_DIR = config.env.DEVENV_STATE + "/keycloak/tmp";

        KC_BOOTSTRAP_ADMIN_PASSWORD = "admin";
        KC_BOOTSTRAP_ADMIN_USERNAME = "${lib.escapeShellArg cfg.initialAdminPassword}";
      };

      processes.keycloak =
        let

          importRealms = lib.optionalString (cfg.realmFiles != [ ]) (
            builtins.concatStringsSep "\n" (
              lib.map (f: ''${cfg.package}/bin/kc.sh import --file "${f}"'') cfg.importRealms
            )
          );

          startScript = pkgs.writeShellScriptBin "start-keycloak" ''
            set -euo pipefail
            mkdir -p "$KC_HOME_DIR"
            mkdir -p "$KC_HOME_DIR/providers"
            mkdir -p "$KC_HOME_DIR/conf"
            mkdir -p "$KC_HOME_DIR/tmp"

            ${importRealms}

            ${cfg.package}/bin/kc.sh show-config
            ${cfg.package}/bin/kc.sh --verbose start --optimized
          '';

        in
        # healthScript = pkgs.writeShellScriptBin "health-keycloak" ''
          #   ${cfg.package}/bin/kcadm.sh config credentials \
          #       --server "http://${cfg.settings.hostname}:${cfg.settings.port}" \
          #       --realm master \
          #       --user "${cfg.initialAdminUsername}" \
          #       --password "${cfg.initialAdminPassword}"
          #
          #   ${cfg.package}/bin/kcadm.sh get "http://${cfg.hostname}:9000"
          # '';
        {
          exec = "exec ${startScript}/bin/start-keycloak";

          # process-compose = {
          #   readiness_probe = {
          #     exec.command = "${postgresPkg}/bin/pg_isready -d template1";
          #     initial_delay_seconds = 10;
          #     period_seconds = 10;
          #     timeout_seconds = 4;
          #     success_threshold = 1;
          #     failure_threshold = 5;
          #   };
          # };
        };
    };
}
