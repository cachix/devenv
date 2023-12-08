{ pkgs, ... }: {
  dotenv.enable = true;
  dotenv.filename = [ ".env" "bar.env" ];

  env.BAR = "1";
}
