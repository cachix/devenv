{ pkgs, config, ... }:

{
  services = {
    nginx = {
      enable = true;
      package = pkgs.nginx;
      httpConfig = ''
        keepalive_timeout  65;

        server {
            listen       8400;
            server_name  _;

            root ${config.env.DEVENV_ROOT};
        }
      '';
    };
  };
}

