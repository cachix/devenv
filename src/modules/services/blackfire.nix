{ pkgs, lib, config, ... }:

let
  cfg = config.services.blackfire;

  configFile = pkgs.writeText "blackfire.conf" ''
    [blackfire]
    server-id=${cfg.server-id}
    server-token=${cfg.server-token}
    socket=${cfg.socket}
  '';
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "blackfire" "enable" ] [ "services" "blackfire" "enable" ])
  ];

  options.services.blackfire = {
    enable = lib.mkEnableOption ''
      Blackfire profiler agent

      It automatically installs Blackfire PHP extension.
    '';

    enableApm = lib.mkEnableOption ''
      Enables application performance monitoring, requires special subscription.
    '';

    client-id = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the client id used to authenticate with Blackfire.
        You can find your personal client-id at <https://blackfire.io/my/settings/credentials>.
      '';
      default = "";
    };

    client-token = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the client token used to authenticate with Blackfire.
        You can find your personal client-token at <https://blackfire.io/my/settings/credentials>.
      '';
      default = "";
    };

    server-id = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the server id used to authenticate with Blackfire.
        You can find your personal server-id at <https://blackfire.io/my/settings/credentials>.
      '';
      default = "";
    };

    server-token = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the server token used to authenticate with Blackfire.
        You can find your personal server-token at <https://blackfire.io/my/settings/credentials>.
      '';
      default = "";
    };

    socket = lib.mkOption {
      type = lib.types.str;
      default = "tcp://127.0.0.1:8307";
      description = ''
        Sets the server socket path
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of blackfire to use";
      default = pkgs.blackfire;
      defaultText = lib.literalExpression "pkgs.blackfire";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.BLACKFIRE_AGENT_SOCKET = cfg.socket;
    env.BLACKFIRE_CLIENT_ID = cfg.client-id;
    env.BLACKFIRE_CLIENT_TOKEN = cfg.client-token;
    env.BLACKFIRE_APM_ENABLED = (if cfg.enableApm then "1" else "0");

    processes.blackfire-agent.exec = "${cfg.package}/bin/blackfire agent:start --config=${configFile}";
  };
}
