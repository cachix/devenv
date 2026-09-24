{ pkgs, ... }:

{
  machines.combined = {
    system = pkgs.stdenv.hostPlatform.system;
    target.host = "root@combined.example.com";
    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      home.stateVersion = "24.11";
    };
  };
}
