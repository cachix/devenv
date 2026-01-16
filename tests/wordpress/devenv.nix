{ pkgs, config, ... }:

{
  packages = [
    pkgs.wp-cli
  ];

  languages.php = {
    enable = true;
    version = "8.2";

    extensions = [
      "mysqli"
      "pdo_mysql"
      "gd"
      "zip"
      "intl"
      "exif"
    ];

    ini = ''
      memory_limit = 256M
    '';

    fpm.pools.web = {
      settings = {
        "pm" = "dynamic";
        "pm.max_children" = 10;
        "pm.start_servers" = 2;
        "pm.min_spare_servers" = 1;
        "pm.max_spare_servers" = 5;
      };
    };
  };

  services.mysql = {
    enable = true;
    package = pkgs.mariadb;
    initialDatabases = [{ name = "wordpress"; }];
    ensureUsers = [{
      name = "wordpress";
      password = "wordpress";
      ensurePermissions = { "wordpress.*" = "ALL PRIVILEGES"; };
    }];
  };

  services.caddy = {
    enable = true;
    virtualHosts."http://localhost:8000" = {
      extraConfig = ''
        root * ${config.devenv.root}
        php_fastcgi unix/${config.languages.php.fpm.pools.web.socket}
        file_server
      '';
    };
  };
}
