{
  services.pgbouncer = {
    enable = true;
    listen_addr = "*";
    port = 6666;
    settings = {
      auth_type = "trust";
      auth_file = toString ./auth_file;
    };

    databases.test = {
      host = "127.0.0.1";
      port = 5555;
    };
    users.test = {
      pool_mode = "transaction";
    };
    peers = {
      "1" = {
        host = "host1.example.com";
      };
      "2" = {
        host = "/tmp/pgbouncer-2";
        port = 5555;
      };
    };
  };

  services.postgres = {
    enable = true;
    listen_addresses = "*";
    port = 5555;
    initialDatabases = [
      {
        name = "test";
        user = "test";
        pass = "123";
      }
    ];
  };
}
