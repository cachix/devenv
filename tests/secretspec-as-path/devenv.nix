{ config, ... }:

{
  # The .env file feeds secretspec's dotenv provider, not the dotenv integration.
  dotenv.disableHint = true;

  env.TLS_KEY_PATH = config.secretspec.secrets.TLS_KEY or "";

  # Records the path it was given, then reads the file while it runs.
  processes.reader = {
    exec = ''
      echo "$TLS_KEY_PATH" > watched-path
      while cat "$TLS_KEY_PATH" > /dev/null; do sleep 1; done
    '';
    ready.exec = "test -s watched-path";
  };
}
