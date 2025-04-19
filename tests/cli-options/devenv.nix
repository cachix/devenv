{ pkgs, config, ... }:

{
  # Default language - will be overridden by CLI option
  languages.rust.enable = true;
  languages.rust.version = "stable";

  # Default services - will be overridden by CLI option
  services.redis.enable = false;

  # Check if the CLI option value was correctly set
  env.RUST_VERSION = config.languages.rust.version;
  env.REDIS_ENABLED = builtins.toString config.services.redis.enable;
}
