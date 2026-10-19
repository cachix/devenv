{ config, pkgs, ... }:

{
  dagger = {
    enable = false;
    package = pkgs.hello;
    dang.enable = true;
    version = "v0.20.0";
    engine = "cloud";
  };

  assertions = [
    {
      assertion = !config.languages.dang.enable;
      message = "A disabled Dagger module should not enable Dang.";
    }
    {
      assertion = !(builtins.elem config.dagger.package config.packages);
      message = "A disabled Dagger module should not install its package.";
    }
    {
      assertion = !(config.env ? DAGGER_X_RELEASE) && !(config.env ? DAGGER_ENGINE);
      message = "A disabled Dagger module should not export its options.";
    }
    {
      assertion = !(config.env ? DO_NOT_TRACK);
      message = "A disabled Dagger module should not set DO_NOT_TRACK.";
    }
  ];
}
