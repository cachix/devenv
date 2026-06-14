{ pkgs, config, lib, ... }:
let
  cfg = config.services.pocket-id;
  types = lib.types;
  pocket-id-storage = config.env.DEVENV_STATE + "/pocket-id";
in
{
  options.services.pocket-id = {
    enable = lib.mkEnableOption "Pocket ID server, an OIDC provider. [pocket-id.org](https://pocket-id.org)";

    package = lib.mkOption {
      type = types.package;
      default = pkgs.pocket-id;
      defaultText = lib.literalExpression "pkgs.pocket-id";
      description = "The pocket-id package to use.";
    };

    app_url = lib.mkOption {
      type = types.str;
      default = "http://localhost:1411";
      description = ''
        Specifies the connection string used to connect to the database.

        This will set the environment variable `APP_URL`.
      '';
    };

    disable_analytics = lib.mkOption {
      type = types.bool;
      description = ''
        Disable heartbeat that gets sent every 24 hours to count how many Pocket ID instances are running.

        See [docs page](https://pocket-id.org/docs/configuration/analytics/).

        This will set the environment variable `ANALYTICS_DISABLED`.
      '';
      default = false;
    };

    disable_geolite = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        Disable usage of GeoLite by setting the download URL for the GeoLite database to an empty string.

        This will set the environment variable `GEOLITE_DB_URL` with an empty string.
      '';
    };

    reverse_proxy = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether the app is behind a reverse proxy.

        This will set the environment variable `TRUST_PROXY`.
      '';
    };

    use_unix_socket = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        Make pocket-id listen to a UNIX socket instead of TCP. The socket will be located at `$DEVENV_RUNTIME/pocket-id.sock`.

        This will set the `UNIX_SOCKET` environment variable with the socket location. Pocket ID will ignore the environment variables `HOST` and `PORT`.

        Additionally, the option `reverse_proxy` will be set to `true`.
      '';
    };

    disable_ui_configuration = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        Disable the ability to configure the UI through the web client. Customization will be done exclusively through environment variables.

        This will set the environment variable `UI_CONFIG_DISABLED`.
      '';
    };

    env = lib.mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = ''
        Additional environment variables for pocket-id.

        See [list of all variables](https://pocket-id.org/docs/configuration/environment-variables).
      '';
    };

  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env = {
      ANALYTICS_DISABLED = if cfg.disable_analytics then "true" else null;

      APP_URL = cfg.app_url;

      DB_CONNECTION_STRING = "file:${pocket-id-storage}/pocket-id.db";
      UPLOAD_PATH = "${pocket-id-storage}/uploads";
      KEYS_PATH = "${pocket-id-storage}/keys";
      GEOLITE_DB_PATH = "${pocket-id-storage}/GeoLite2-City.mmdb";

      UNIX_SOCKET = if cfg.use_unix_socket then "${config.env.DEVENV_RUNTIME}/pocket-id.sock" else null;
      TRUST_PROXY = if cfg.use_unix_socket or cfg.reverse_proxy then "true" else null;

      GEOLITE_DB_URL = if cfg.disable_geolite then "" else null;

      UI_CONFIG_DISABLED = if cfg.disable_ui_configuration then "true" else null;
    } // cfg.env;

    processes.pocket-id.exec = ''
      mkdir -p ${pocket-id-storage}
      exec "${cfg.package}/bin/pocket-id"
    '';
  };

}
