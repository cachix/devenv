{ pkgs, ... }: {
  dotenv.enable = true;

  env.BAR = "1";
}
