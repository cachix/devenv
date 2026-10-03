{ config, ... }:

{
  # The .env file feeds secretspec's dotenv provider, not the dotenv integration.
  dotenv.disableHint = true;

  env.TLS_KEY_PATH = config.secretspec.secrets.TLS_KEY or "";
}
