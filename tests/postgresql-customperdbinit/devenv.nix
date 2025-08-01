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
        user = "testuser";
        pass = "testuserpass";
        initialSQL = ''
          CREATE EXTENSION IF NOT EXISTS pg_uuidv7;
          CREATE TABLE user_owned_table (id SERIAL PRIMARY KEY, name TEXT);
          ALTER TABLE user_owned_table OWNER TO testuser;
        '';
      }
      {
        name = "testdb2";
        pass = "testuserpass";
      }
    ];
  };
}
