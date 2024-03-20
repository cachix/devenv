{ pkgs, lib, config, ... }: {
  dotenv.enable = true;

  env.BAR = "1";
}
