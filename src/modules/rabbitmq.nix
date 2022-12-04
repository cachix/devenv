{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.rabbitmq;

  inherit (builtins) concatStringsSep;

  config_file_content = lib.generators.toKeyValue { } cfg.configItems;
  config_file = pkgs.writeText "rabbitmq.conf" config_file_content;

  plugin_file = pkgs.writeText "enabled_plugins" ''
    [ ${concatStringsSep "," cfg.plugins} ].
  '';
in

{
  options = {
    rabbitmq = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Whether to enable the RabbitMQ server, an Advanced Message
          Queuing Protocol (AMQP) broker.
        '';
      };

      package = mkOption {
        default = pkgs.rabbitmq-server;
        type = types.package;
        defaultText = literalExpression "pkgs.rabbitmq-server";
        description = ''
          Which rabbitmq package to use.
        '';
      };

      listenAddress = mkOption {
        default = "127.0.0.1";
        example = "";
        description = ''
          IP address on which RabbitMQ will listen for AMQP
          connections.  Set to the empty string to listen on all
          interfaces.  Note that RabbitMQ creates a user named
          <literal>guest</literal> with password
          <literal>guest</literal> by default, so you should delete
          this user if you intend to allow external access.
          Together with 'port' setting it's mostly an alias for
          configItems."listeners.tcp.1" and it's left for backwards
          compatibility with previous version of this module.
        '';
        type = types.str;
      };

      port = mkOption {
        default = 5672;
        description = ''
          Port on which RabbitMQ will listen for AMQP connections.
        '';
        type = types.port;
      };

      cookie = mkOption {
        default = "";
        type = types.str;
        description = ''
          Erlang cookie is a string of arbitrary length which must
          be the same for several nodes to be allowed to communicate.
          Leave empty to generate automatically.
        '';
      };

      configItems = mkOption {
        default = { };
        type = types.attrsOf types.str;
        example = literalExpression ''
          {
            "auth_backends.1.authn" = "rabbit_auth_backend_ldap";
            "auth_backends.1.authz" = "rabbit_auth_backend_internal";
          }
        '';
        description = ''
          Configuration options in RabbitMQ's new config file format,
          which is a simple key-value format that can not express nested
          data structures. This is known as the <literal>rabbitmq.conf</literal> file,
          although outside NixOS that filename may have Erlang syntax, particularly
          prior to RabbitMQ 3.7.0.
          If you do need to express nested data structures, you can use
          <literal>config</literal> option. Configuration from <literal>config</literal>
          will be merged into these options by RabbitMQ at runtime to
          form the final configuration.
          See https://www.rabbitmq.com/configure.html#config-items
          For the distinct formats, see https://www.rabbitmq.com/configure.html#config-file-formats
        '';
      };

      plugins = mkOption {
        default = [ ];
        type = types.listOf types.str;
        description = "The names of plugins to enable";
      };

      pluginDirs = mkOption {
        default = [ ];
        type = types.listOf types.path;
        description = "The list of directories containing external plugins";
      };

      managementPlugin = {
        enable = mkEnableOption "the management plugin";
        port = mkOption {
          default = 15672;
          type = types.port;
          description = ''
            On which port to run the management plugin
          '';
        };
      };
    };
  };

  config = mkIf config.rabbitmq.enable {
    packages = [ cfg.package ];

    rabbitmq.configItems = {
      "listeners.tcp.1" = mkDefault "${cfg.listenAddress}:${toString cfg.port}";
    } // optionalAttrs cfg.managementPlugin.enable {
      "management.tcp.port" = toString cfg.managementPlugin.port;
      "management.tcp.ip" = cfg.listenAddress;
    };

    rabbitmq.plugins = optional cfg.managementPlugin.enable "rabbitmq_management";

    env.RABBITMQ_DATA_DIR = config.env.DEVENV_STATE + "/rabbitmq";
    env.RABBITMQ_MNESIA_BASE = config.env.RABBITMQ_DATA_DIR + "/mnesia";
    env.RABBITMQ_LOGS = "-";
    env.RABBITMQ_LOG_BASE = config.env.RABBITMQ_DATA_DIR + "/logs";
    env.RABBITMQ_CONFIG_FILE = config_file;
    env.RABBITMQ_PLUGINS_DIR = concatStringsSep ":" cfg.pluginDirs;
    env.RABBITMQ_ENABLED_PLUGINS_FILE = plugin_file;

    processes.rabbitmq.exec = "${cfg.package}/bin/rabbitmq-server";
  };
}
