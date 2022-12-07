{ pkgs, config, ... }:

let
  phpPackage = pkgs.php.buildEnv {
    extraConfig = ''
      memory_limit = 256M
    '';
  };
in
{
  languages.php.enable = true;
  languages.php.package = phpPackage;
  languages.php.fpm.pools.web = {
    settings = {
      "pm" = "dynamic";
      "pm.max_children" = 5;
      "pm.start_servers" = 2;
      "pm.min_spare_servers" = 1;
      "pm.max_spare_servers" = 5;
    };
  };

  caddy.enable = true;
  caddy.virtualHosts."http://localhost:8000" = {
    extraConfig = ''
      root * public
      php_fastcgi unix/${config.languages.php.fpm.pools.web.socket}
      file_server
    '';
  };
}
