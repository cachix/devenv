{ pkgs, ... }:

{
  services.rabbitmq = {
    enable = true;
    managementPlugin = { enable = true; };
  };
}
