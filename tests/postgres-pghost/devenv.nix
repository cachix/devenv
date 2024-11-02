{
  services.postgres = {
    enable = true;
    listen_addresses = "*";
    port = 2345;
    initialScript = ''
      CREATE USER postgres SUPERUSER;
    '';
  };
}
