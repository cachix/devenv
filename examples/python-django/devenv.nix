{ pkgs, config, ... }:

let
  db_user = "postgres";
  db_host = "localhost";
  db_port = "5432";
  db_name = "db";
in
{
  packages = [ pkgs.git pkgs.postgresql_14 ];

  languages.python = {
    enable = true;
    package = pkgs.python310;
    poetry.enable = true;
  };

  env = {
    DATABASE_URL = "postgres://${db_user}@/${db_name}?host=${config.env.PGHOST}";
    DEBUG = true;
    STATIC_ROOT = "/tmp/static";
  };

  services.postgres = {
    enable = true;
    initialScript = "CREATE USER ${db_user} SUPERUSER;";
    initialDatabases = [{ name = db_name; }];
    listen_addresses = db_host;
  };

  processes = {
    runserver.exec = ''
      devenv shell python manage.py runserver
    '';
  };

  scripts = {
    start-processes-in-background.exec = ''
      echo
      psql --version

      # Start Postgres if not running ...
      if ! nc -z ${db_host} ${db_port};
      then
        echo "Starting Database in the background on ${db_host}:${db_port} ..."
        nohup devenv up > /tmp/devenv.log 2>&1 &
      fi
    '';
    wait-for-db.exec = ''
      echo
      echo "Waiting for database to start .."
      echo "(if wait exceeds 100%, check /tmp/devenv.log for errors!)"
      
      timer=0;
      n_steps=99;
      while true;
      do
        if nc -z ${db_host} ${db_port}; then
          printf "\nDatabase is running!\n\n"
          exit 0
        elif [ $timer -gt $n_steps ]; then
          printf "\nDatabase failed to launch!\n\n"
          exit 1
        else
          sleep 0.1
          let timer++
          printf "%-*s" $((timer+1)) '[' | tr ' ' '#'
          printf "%*s%3d%%\r"  $((100-timer))  "]" "$timer"
        fi
      done
    '';
    run-tests.exec = ''
      start-processes-in-background
      wait-for-db || exit 1
      python manage.py collectstatic --noinput
      python manage.py test
    '';
  };
}
