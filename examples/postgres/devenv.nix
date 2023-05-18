{ ... }:

{
  services.postgres.enable = true;

  # Enable the optional PostGIS extension.
  services.postgres.extensions = extensions: [ extensions.postgis ];
  services.postgres.initialScript = ''
    CREATE EXTENSION IF NOT EXISTS postgis;
  '';
}
