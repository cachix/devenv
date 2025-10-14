# Wordpress

```nix title="devenv.nix"
{ pkgs, config, ... }:

{
  packages = with pkgs;[
    git
    wp-cli
  ];

  languages.php.enable = true;
  languages.php.package = pkgs.php82.buildEnv {
    extensions = { all, enabled }: with all; enabled ++ [ redis pdo_mysql xdebug ];
    extraConfig = ''
      memory_limit = -1
      xdebug.mode = debug
      xdebug.start_with_request = yes
      xdebug.idekey = vscode
      xdebug.log_level = 0
      max_execution_time = 0
    '';
  };

  languages.php.fpm.pools.web = {
    settings = {
      "clear_env" = "no";
      "pm" = "dynamic";
      "pm.max_children" = 10;
      "pm.start_servers" = 2;
      "pm.min_spare_servers" = 1;
      "pm.max_spare_servers" = 10;
    };
  };

  certificates = [
    "wp.localhost"
  ];

  # This lets Caddy bind to 443
  scripts.caddy-setcap.exec = ''
    sudo setcap 'cap_net_bind_service=+ep' ${pkgs.caddy}/bin/caddy
  '';
  services.redis.enable = true;

  # Links to MariaDB internally
  services.mysql = {
    enable = true;
    settings.mysqld = {
      max_allowed_packet = "512M";
    };
  };

  services.mysql.initialDatabases = [{name = "wp"; }];
  services.mysql.ensureUsers = [
    {
      name = "wordpress";
      password = "YourSecretSauceHere";
      ensurePermissions = { "wp.*" = "ALL PRIVILEGES"; };
    }
  ];

  services.caddy.enable = true;
  services.caddy.virtualHosts."wp.localhost" = {
    extraConfig = ''
      tls ${config.env.DEVENV_STATE}/mkcert/wp.localhost.pem ${config.env.DEVENV_STATE}/mkcert/wp.localhost-key.pem
      root * .
      php_fastcgi unix/${config.languages.php.fpm.pools.web.socket}
      file_server
    '';
  };
}
```
