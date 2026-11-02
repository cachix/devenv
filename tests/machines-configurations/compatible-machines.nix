{ inputs }:
{ lib, ... }:
{
  key = "devenv-machines";

  imports = [
    (inputs.devenv.modules + /machines/recovery.nix)
    (inputs.devenv.modules + /machines/facts.nix)
    (inputs.devenv.modules + /machines/secrets.nix)
  ];

  options.devenv.machinesVersion = lib.mkOption {
    type = lib.types.ints.positive;
    internal = true;
    readOnly = true;
  };

  config = {
    devenv.machinesVersion = 1;
    environment.etc."module-version-marker".text = "from the pinned modules";
  };
}
