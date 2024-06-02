{ pkgs, ... }:

{
  languages.terraform = {
    enable = true;
    version = "1.8.4";
  };
}
