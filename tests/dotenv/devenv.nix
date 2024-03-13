{ pkgs, ... }: {
  dotenv.enable = true;
  dotenv.filename = [ ".env" ".env.bar" ];

  env.BAR = "1";
}
