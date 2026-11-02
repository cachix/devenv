{ lib, ... }:
{
  key = "devenv-machines";

  options.devenv.machinesVersion = lib.mkOption {
    type = lib.types.ints.positive;
    internal = true;
  };

  config.devenv.machinesVersion = 999;
}
