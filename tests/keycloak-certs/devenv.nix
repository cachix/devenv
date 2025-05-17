{ pkgs, ... }:

{
  services.keycloak = {
    enable = true;
    settings.http-port = 8089;

    database.type = "dev-file";

    sslCertificate = "./certs/ssl-cert.crt";
    sslCertificateKey = "./certs/ssl-cert.key";

    realms = {
      master = {
        path = "./realms/master.json";
        export = true;
        import = false;
      };
      test = {
        path = "./realms/test.json";
        # import = true;
        export = true;
      };
    };
  };

  packages = [
    pkgs.curl
    pkgs.process-compose
  ];
}
