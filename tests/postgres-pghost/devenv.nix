{
  services.postgres = {
    enable = true;
    listen_addresses = "*";
    # Give this wildcard listener its own base port on shared runners.
    port = 24435;
    initialScript = ''
      CREATE USER postgres SUPERUSER;
    '';
  };
}
