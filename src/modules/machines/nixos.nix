{ lib, ... }:
{
  key = "devenv-machines";

  imports = [
    ./recovery.nix
    ./facts.nix
    ./secrets.nix
  ];

  options.devenv.machinesVersion = lib.mkOption {
    type = lib.types.ints.positive;
    internal = true;
    readOnly = true;
    description = "Version of the devenv machine module interface.";
  };

  config.devenv.machinesVersion = import ./interface-version.nix;
}
