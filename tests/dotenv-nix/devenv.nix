{ config, ... }:
{
  dotenv.enable = true;
  dotenv.filename = [
    ".env"
    ".env.local"
  ];
  dotenv.substitution = true;

  env.DERIVED_IN_NIX = "${config.env.BASE}-${config.env.QUOTED}";
  env.HOST_DERIVED_IN_NIX = "${config.env.FROM_HOST}-nix";
  env.LOCAL_DERIVED_IN_NIX = if config.env ? LOCAL then "${config.env.LOCAL}-nix" else "missing";
}
