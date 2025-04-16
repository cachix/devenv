{ pkgs, ... }:

{
  services.keycloak = {
    enable = true;
    settings.http-port = 8089;

    database.type = "dev-file";

    realms = {
      master = {
        path = "./realms/master.json";
        export = true;
        import = false;
      };
      test = {
        path = "./realms/test.json";
        export = true;
      };
    };
  };

  packages = [
    pkgs.curl
    pkgs.process-compose
  ];
}
