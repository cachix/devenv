{ pkgs, lib, config, ... }:

let
  cfg = config.blackfire;

  configFile = pkgs.writeText "blackfire.conf" ''
    [blackfire]
    server-id=${cfg.server-id}
    server-token=${cfg.server-token}
    socket=${cfg.socket}
  '';
in
{
  options.blackfire = {
    enable = lib.mkEnableOption (lib.mdDoc ''
      Blackfire profiler agent

      For PHP you need to install and configure the Blackfire PHP extension.

      ```nix
      languages.php.package = pkgs.php.buildEnv {
        extensions = { all, enabled }: with all; enabled ++ [ (blackfire// { extensionName = "blackfire"; }) ];
        extraConfig = '''
          memory_limit = 256M
          blackfire.agent_socket = "${config.blackfire.socket}";
        ''';
      };
      ```
    '');

    client-id = lib.mkOption {
      type = lib.types.str;
      description = lib.mdDoc ''
        Sets the client id used to authenticate with Blackfire
        You can find your personal client-id at https://blackfire.io/my/settings/credentials
      '';
      default = "";
    };

    client-token = lib.mkOption {
      type = lib.types.str;
      description = lib.mdDoc ''
        Sets the client token used to authenticate with Blackfire
        You can find your personal client-token at https://blackfire.io/my/settings/credentials
      '';
      default = "";
    };

    server-id = lib.mkOption {
      type = lib.types.str;
      description = lib.mdDoc ''
        Sets the server id used to authenticate with Blackfire
        You can find your personal server-id at https://blackfire.io/my/settings/credentials
      '';
      default = "";
    };

    server-token = lib.mkOption {
      type = lib.types.str;
      description = lib.mdDoc ''
        Sets the server token used to authenticate with Blackfire
        You can find your personal server-token at https://blackfire.io/my/settings/credentials
      '';
      default = "";
    };

    socket = lib.mkOption {
      type = lib.types.str;
      default = "tcp://127.0.0.1:8307";
      description = lib.mdDoc ''
        Sets the server socket path
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of blackfire to use";
      default = pkgs.blackfire;
      defaultText = "pkgs.blackfire";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.BLACKFIRE_AGENT_SOCKET = cfg.socket;
    env.BLACKFIRE_CLIENT_ID = cfg.client-id;
    env.BLACKFIRE_CLIENT_TOKEN = cfg.client-token;

    processes.blackfire-agent.exec = "${cfg.package}/bin/blackfire agent:start --config=${configFile}";
  };
}
