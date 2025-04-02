{ config
, lib
, pkgs
, ...
}:

let
  cfg = config.services.keycloak;

  inherit (lib)
    mdDoc
    mkIf
    mkOption
    mkRenamedOptionModule
    escapeShellArg
    literalExpression
    types
    ;
in
{
  imports = [
    (mkRenamedOptionModule [ "keycloak" "enable" ] [ "services" "keycloak" "enable" ])
  ];

  options.services.keycloak = {
    enable = mkOption {
      description = "Whether to enable keycloak.";
      default = false;
      type = types.bool;
    };

    initialAdminUsername = mkOption {
      type = types.str;
      default = "admin";
      description = mdDoc ''
        Initial admin user name.
        See [`KC_BOOTSTRAP_ADMIN_USERNAME`](https://www.keycloak.org/server/all-config?f=config)
      '';
    };

    initialAdminPassword = mkOption {
      type = types.str;
      default = "admin";
      description = mdDoc ''
        Initial password set for the `admin`
        user. The password is not stored safely and should be changed
        immediately in the admin panel.
        See [`KC_BOOTSTRAP_ADMIN_PASSWORD`](https://www.keycloak.org/server/all-config?f=config)
      '';
    };

    hostname = lib.mkOption {
      type = types.str;
      default = "localhost";
      description = ''
        Address at which is the server exposed.
        See [`KC_HOSTNAME`](https://www.keycloak.org/server/all-config?f=config)
      '';
    };

    port = lib.mkOption {
      type = types.port;
      default = 8080;
      description = ''
        The HTTP port to accept connections.
        See [`KC_HTTP_PORT`](https://www.keycloak.org/server/all-config?f=config)
      '';
    };

    initialImportFile = lib.mkOption {
      type = types.nullOr types.path;
      default = null;
      description = ''
        The initial import JSON file for keycloak. You can use:
        `kc.sh export --realm <your-realm> --file export.json`
      '';
    };

    package = mkOption {
      description = "Keycloak package to use.";
      default = pkgs.keycloak;
      defaultText = literalExpression "pkgs.keycloak";
      type = types.package;
    };
  };

  config = mkIf cfg.enable {
    packages = [ cfg.package ];

    env.KC_DB = "dev-mem";

    env.KC_HOME_DIR = config.env.DEVENV_STATE + "/keycloak";
    env.KC_CONF_DIR = config.env.DEVENV_STATE + "/keycloak/conf";
    env.KC_TMP_DIR = config.env.DEVENV_STATE + "/keycloak/tmp";

    env.KC_BOOTSTRAP_ADMIN_PASSWORD = cfg.initialAdminUsername;
    env.KC_BOOTSTRAP_ADMIN_USERNAME = "${escapeShellArg cfg.initialAdminPassword}";

    env.KC_HEALTH_ENABLE = "true";

    env.KC_HOSTNAME = cfg.hostname;
    env.KC_HTTP_PORT = cfg.port;

    env.KC_LOG_CONSOLE_COLOR = "true";
    env.KC_LOG_LEVEL = "debug";
    env.KC_LOG = "console";

    processes.keycloak =
      let
        startScript = pkgs.writeShellScriptBin "start-keycloak" (
          ''
            set -euo pipefail
            mkdir -p "$KC_HOME_DIR"
            mkdir -p "$KC_HOME_DIR/providers"
            mkdir -p "$KC_HOME_DIR/conf"
            mkdir -p "$KC_HOME_DIR/tmp"

            exeDir="${cfg.package}"

            # Try to copy to local folder, (probably wrong, needs nix copy??)
            # exeDir="$KC_HOME_DIR/build-temp"
            # mkdir -p "$KC_HOME_DIR/build-temp"
            # cp -rf --no-preserve=ownership "${cfg.package}/." "$KC_HOME_DIR/build-temp/"
            # chmod -R u+w "$exeDir"

          ''
          + (lib.optionalString (cfg.initialImportFile != null) ''
            "$exeDir/bin/kc.sh" import \
              --file "${cfg.initialImportFile}" \
          '')
          + ''
            "$exeDir/bin/kc.sh" show-config
            "$exeDir/bin/kc.sh" --verbose start-dev
          ''
        );

        healthScript = pkgs.writeShellScriptBin "health-keycloak" ''
          ${cfg.package}/bin/kcadm.sh config credentials \
              --server "http://${cfg.hostname}:${cfg.port}" \
              --realm master \
              --user "${cfg.initialAdminUsername}" \
              --password "${cfg.initialAdminPassword}"

          ${cfg.package}/bin/kcadm.sh get "http://${cfg.hostname}:9000"
        '';
      in
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
