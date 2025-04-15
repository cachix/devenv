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
          "dev-file"
        ];
        default = "dev-file";
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
      apply = x: lib.map (assertStringPath "realmFiles") x;
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

    realmExport = mkOption {
      default = { };
      type = types.attrsOf (
        types.submodule {
          options = {
            path = mkOption {
              type = nullOr types.path;
              default = null;
              example = "./realms/a.json";
              description = ''
                The path where you want to export this realm «name» to.
                If not set its exported to `$DEVENV_STATE/keycloak/realm-export/«name»`.
              '';
            };
          };
        }
      );

      example = lib.literalExpression ''
        {
          myrealm.path = "./myfolder/export.json";
        }
      '';

      description = ''
        Specify the realms you want to export on a process 'keycloak-export-realms'
        which you can launch manually. If the path is not specified they are exported
        to directory `$DEVENV_STATE/keycloak/realm-export`.
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

          # https-port = mkOption {
          #   type = types.port;
          #   default = 443;
          #   example = 8443;
          #   description = ''
          #     On which port Keycloak should listen for new HTTPS connections.
          #   '';
          # };

          # http-relative-path = mkOption {
          #   type = types.str;
          #   default = "/";
          #   example = "/auth";
          #   apply = x: if !(lib.hasPrefix "/") x then "/" + x else x;
          #   description = ''
          #     The path relative to `/` for serving
          #     resources.
          #
          #     ::: {.note}
          #     In versions of Keycloak using Wildfly (&lt;17),
          #     this defaulted to `/auth`. If
          #     upgrading from the Wildfly version of Keycloak,
          #     i.e. a NixOS version before 22.05, you'll likely
          #     want to set this to `/auth` to
          #     keep compatibility with your clients.
          #
          #     See <https://www.keycloak.org/migration/migrating-to-quarkus>
          #     for more information on migrating from Wildfly to Quarkus.
          #     :::
          #   '';
          # };

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

      dummyCertificates = pkgs.stdenv.mkDerivation {
        pname = "dev-ssl-cert";
        version = "1.0";
        buildInputs = [ pkgs.openssl ];
        src = null;
        dontUnpack = true;
        buildPhase = ''
          mkdir -p $out
          openssl req -x509 -newkey rsa:2048 -nodes \
            -keyout $out/ssl-cert.key -out $out/ssl-cert.crt \
            -days 365 \
            -subj "/CN=localhost"
        '';

        installPhase = "true";
      };

      providedSSLCerts = cfg.sslCertificate != null && cfg.sslCertificateKey != null;
    in
    mkIf cfg.enable {

      services.keycloak.settings = mkMerge [
        {
          # We always enable http since we also use it to check the health.
          http-enabled = true;
          db = cfg.database.type;

          health-enable = true;

          log-console-level = "debug";
          log-level = "debug";
        }
        (mkIf providedSSLCerts {
          https-certificate-file = cfg.sslCertificate;
          https-certificate-key-file = cfg.sslCertificateKey;
        })
        (mkIf (!providedSSLCerts) {
          https-certificate-file = "${dummyCertificates}/ssl-cert.crt";
          https-certificate-key-file = "${dummyCertificates}/ssl-cert.key";
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
          importRealms = lib.optional (cfg.realmFiles != [ ]) (
            lib.map
              (f: ''
                echo "Importing realm file '${f}'."
                ${keycloakBuild}/bin/kc.sh import --file "${f}"
              '')
              cfg.importRealms
          );

          keycloak-start = pkgs.writeShellScriptBin "keycloak-start" ''
            set -euo pipefail
            mkdir -p "$KC_HOME_DIR"
            mkdir -p "$KC_HOME_DIR/providers"
            mkdir -p "$KC_HOME_DIR/conf"
            mkdir -p "$KC_HOME_DIR/tmp"

            # Install config file.
            # install -D -m 0600 ${confFile} "$KC_HOME_DIR/conf/keycloak.conf"

            ${builtins.concatStringsSep "\n" importRealms}

            ${keycloakBuild}/bin/kc.sh show-config || true # >/persist/repos/devenv/test.log 2>&1
            ${keycloakBuild}/bin/kc.sh --verbose start-dev # >>/persist/repos/devenv/test.log 2>&1
          '';

          # We could use `kcadm.sh get "http://localhost:9000"` but that needs
          # credentials, so we just check the master realm.
          keycloak-health =
            let
              host = cfg.settings.hostname + ":" + builtins.toString cfg.settings.http-port;
            in
            pkgs.writeShellScriptBin "keycloak-health" ''
              ${pkgs.curl} -v \
                "http://${host}/auth/realms/master/.well-known/openid-configuration"
            '';
        in
        {
          exec = "exec ${keycloak-start}/bin/keycloak-start";

          process-compose = {
            description = "The keycloak identity and access management server.";
            readiness_probe = {
              exec.command = "${keycloak-health}/bin/keycloak-health";
              initial_delay_seconds = 20;
              period_seconds = 10;
              timeout_seconds = 4;
              success_threshold = 1;
              failure_threshold = 5;
            };
          };
        };

      processes.keycloak-export-realms =
        let
          # Generate the command to export the realms.
          realmExports = lib.optional (cfg.realmExport != { }) (
            lib.mapAttrsToList
              (
                realm: e:
                  let
                    file =
                      if e.path == null then
                        (config.env.DEVENV_STATE + "/keycloak/realm-export/${realm}.json")
                      else
                        e.path;
                  in
                  ''
                    echo "Exporting realm '${realm}' to '${file}'."
                    mkdir -p "$(dirname "${file}")"
                    ${keycloakBuild}/bin/kc.sh export --realm "${realm}" --file "${file}"
                  ''
              )
              cfg.exportRealms
          );

          keycloak-realm-export = pkgs.writeShellScriptBin "keycloak-realm-export" ''
            ${lib.concatStringsSep "\n" realmExports}
          '';
        in
        mkIf (cfg.realmExport != { }) {
          exec = "${keycloak-realm-export}/bin/keycloak-realm-export";
          process-compose = {
            description = ''
              Save the realms from keycloak, to back them up. You can run it manually.
            '';
            disabled = true;
          };
        };
    };
}
