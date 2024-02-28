{ pkgs, ... }:

{
  # https://devenv.sh/languages/
  languages.ruby = {
    enable = true;
    version = "2.6.5";
  };
}
