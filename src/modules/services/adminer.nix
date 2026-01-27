{ pkgs, lib, config, ... }:

let
  cfg = config.services.adminer;
  types = lib.types;

  # Port allocation: extract port from listen address or use default
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  basePort = parsePort cfg.listen;
  allocatedPort = config.processes.adminer.ports.main.value;
  host = parseHost cfg.listen;
  listenAddr = "${host}:${toString allocatedPort}";
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "adminer" "enable" ] [ "services" "adminer" "enable" ])
  ];

  options.services.adminer = {
    enable = lib.mkEnableOption "Adminer process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of Adminer to use.";
      default = pkgs.adminer;
      defaultText = lib.literalExpression "pkgs.adminer";
    };

    listen = lib.mkOption {
      type = types.str;
      description = "Listen address for the Adminer.";
      default = "127.0.0.1:8080";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.adminer.ports.main.allocate = basePort;
    processes.adminer.exec = "exec ${config.languages.php.package}/bin/php ${lib.optionalString config.services.mysql.enable "-dmysqli.default_socket=${config.env.MYSQL_UNIX_PORT}"} -S ${listenAddr} -t ${cfg.package} ${cfg.package}/adminer.php";
  };
}
