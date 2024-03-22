{ pkgs, ... }:

{
  languages.terraform = {
    enable = true;
    version = "1.7.4";
  };
}
