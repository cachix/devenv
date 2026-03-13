{ pkgs, ... }:

{
  # TODO: upgrade to native process manager
  process.manager.implementation = "process-compose";

  services.keycloak = {
    enable = true;
    settings.http-port = 8089;

    database.type = "dev-file";

    realms = {
      test = {
        path = "./realms/test.json";
        import = true;
      };
    };
  };

  packages = [
    pkgs.curl
    pkgs.process-compose
  ];
}
