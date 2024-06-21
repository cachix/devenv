{ pkgs, config, ... }:

let
  db_user = "postgres";
  db_name = "db";
in
{
  languages.python = {
    enable = true;
    version = "3.11";
    poetry.enable = true;
  };

  # To load secrets like SECRET_KEY from .env
  # dotenv.enable = true;

  env = {
    DATABASE_URL = "postgres://${db_user}@/${db_name}?host=${config.env.PGHOST}";
    DEBUG = true;
    SECRET_KEY = "supersecret";
    STATIC_ROOT = config.devenv.state + "/static";
  };

  services.postgres = {
    enable = true;
    initialScript = "CREATE USER ${db_user} SUPERUSER;";
    initialDatabases = [{ name = db_name; }];
  };

  processes.runserver = {
    exec = "python manage.py runserver";
    process-compose.depends_on.postgres.condition = "process_healthy";
  };

  enterTest = ''
    python manage.py test
  '';
}
