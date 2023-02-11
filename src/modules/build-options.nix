{ lib, ... }:

let
  types = lib.types;
in
{
  options = {
    derivation = lib.mkOption {
      type = types.package;
      description = "Derivation to build when the build is performed.";
    };
  };
}
