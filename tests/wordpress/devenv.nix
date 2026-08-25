{ pkgs, config, ... }:

let
  caddyPort = config.processes.caddy.ports.http.value;
  mysqlPort = config.processes.mysql.ports.main.value;
in
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
    initialDatabases = [ { name = "wordpress"; } ];
    ensureUsers = [
      {
        name = "wordpress";
        password = "wordpress";
        ensurePermissions = {
          "wordpress.*" = "ALL PRIVILEGES";
        };
      }
    ];
  };

  services.caddy = {
    enable = true;
    config = ''
      {
        admin off
      }
    '';
    virtualHosts."http://127.0.0.1:${toString caddyPort}" = {
      extraConfig = ''
        root * ${config.devenv.root}
        php_fastcgi unix/${config.languages.php.fpm.pools.web.socket}
        file_server
      '';
    };
  };

  files."index.php".text = ''
    <?php
    $conn = new mysqli('127.0.0.1', 'wordpress', 'wordpress', 'wordpress', ${toString mysqlPort});
    if ($conn->connect_error) {
        http_response_code(500);
        die("DB error: " . $conn->connect_error);
    }
    $conn->close();
    echo "OK";
  '';

  processes.phpfpm-web.ready = {
    exec = "test -S ${config.languages.php.fpm.pools.web.socket}";
    initial_delay = 1;
  };

  processes.caddy = {
    ports.http.allocate = 8000;
    after = [ "devenv:processes:phpfpm-web" ];
    ready = {
      http.get = {
        host = "127.0.0.1";
        port = caddyPort;
        path = "/index.php";
      };
      initial_delay = 1;
      probe_timeout = 4;
      failure_threshold = 15;
    };
  };

  enterTest = ''
    export WORDPRESS_HTTP_PORT=${toString caddyPort}
  '';
}
