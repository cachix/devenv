{
  services.postgres = {
    enable = true;
    listen_addresses = "localhost";
    port = 2345;
    initialScript = ''
      CREATE USER postgres SUPERUSER;
    '';
  };
}
