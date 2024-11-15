{ pkgs, ... }:

{
  services.kafka = {
    enable = true;
    connect = {
      enable = true;
    };
  };
}
