{
  services.postgres = {
    enable = true;
    listen_addresses = "localhost";
    port = 2345;
    # NOTE: use default for initialScript, which is:
    # initialScript = ''
    #   CREATE USER postgres SUPERUSER;
    # '';
    initialDatabases = [
      {
        name = "testdb";
        user = "testuser";
        pass = "testuserpass";
        schema = ./.; # *.sql in version order
      }
    ];
  };
}
