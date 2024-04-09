{ pkgs, config, ... }:

let
  db_user = "postgres";
  db_host = "localhost";
  db_port = "5432";
  db_name = "db";
in
{
  languages.python = {
    enable = true;
    version = "3.11";
    poetry.enable = true;
  };

  env = {
    DATABASE_URL = "postgres://${db_user}@/${db_name}?host=${config.env.PGHOST}";
    DEBUG = true;
    SECRET_KEY = "123";
    STATIC_ROOT = "/tmp";
  };

  services.postgres = {
    enable = true;
    initialScript = "CREATE USER ${db_user} SUPERUSER;";
    initialDatabases = [{ name = db_name; }];
    listen_addresses = db_host;
  };

  processes.runserver.exec = "python manage.py runserver";
  processes.runserver.process-compose.depends_on.postgres.condition = "process_ready";

  enterTest = ''
    wait_for_port ${db_port}
    python manage.py test
  '';
}
