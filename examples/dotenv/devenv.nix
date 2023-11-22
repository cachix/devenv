{ pkgs, ... }: {
  dotenv.enable = true;
  dotenv.filenames = [ ".env" ".env.bar" ];

  env.BAR = "1";
}
