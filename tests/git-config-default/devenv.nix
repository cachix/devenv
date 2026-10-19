{ config, pkgs, ... }:
{
  packages = [ pkgs.git ];
  assertions = [
    {
      assertion = !(config.env ? GIT_CONFIG_COUNT);
      message = "Empty Git settings must not inject runtime configuration.";
    }
    {
      assertion = !(config.env ? GIT_CONFIG_GLOBAL);
      message = "Empty Git settings must not override global configuration.";
    }
  ];
}
