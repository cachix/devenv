{ pkgs, lib, config, ... }:

let
  cfg = config.services.adminer;
  types = lib.types;
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
    processes.adminer.exec = "${config.languages.php.package}/bin/php ${lib.optionalString config.services.mysql.enable "-dmysqli.default_socket=${config.env.MYSQL_UNIX_PORT}"} -S ${cfg.listen} -t ${cfg.package} ${cfg.package}/adminer.php";
  };
}
