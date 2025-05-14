{
  services.postgres = {
    enable = true;
    listen_addresses = "localhost";
    port = 2345;
    # NOTE: use default for initialScript, which is:
    # initialScript = ''
    #   CREATE USER postgres SUPERUSER;
    # '';
    extensions = extensions: [
      extensions.pg_uuidv7
    ];

    initialDatabases = [
      {
        name = "testdb";
        pass = "testuserpass";
        initialScript = ''
          CREATE EXTENSION IF NOT EXISTS pg_uuidv7;
        '';
      }
      {
        name = "testdb2";
        pass = "testuserpass";
      }
    ];
  };
}
