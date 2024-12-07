{
  services.postgres = {
    enable = true;
    listen_addresses = "localhost";
    port = 2345;
    initialScript = ''
      CREATE USER postgres SUPERUSER;
    '';
    setupSchemaScript = ''
      echo "script to run to setup or update database schema. This script must be idempotent."
    '';
  };
}
