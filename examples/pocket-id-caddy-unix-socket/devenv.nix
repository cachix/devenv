{ pkgs, config, ... }:
{
  packages = [ pkgs.curl ]; # used for the test

  services = {
    pocket-id = {
      enable = true;
      package = pkgs.pocket-id;

      disable_analytics = true;

      # Use caddy to expose pocket-id to the network
      app_url = "http://localhost";
      use_unix_socket = true;

      # Define any environment variable
      env.APP_NAME = "Devenv";
    };

    caddy = {
      enable = true;
      virtualHosts = {
        "localhost:80".extraConfig = ''
          reverse_proxy unix/${config.env.DEVENV_RUNTIME}/pocket-id.sock
        '';
      };

    };
  };
}
