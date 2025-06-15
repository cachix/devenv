{ pkgs, ... }:

{
  languages.opentofu = {
    enable = true;
    version = "1.9.1";
  };
}
