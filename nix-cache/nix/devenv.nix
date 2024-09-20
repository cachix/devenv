{ lib, ... }:

let
  inherit (lib) types;
in
{
  options = {
    trackedPath = lib.mkOption {
      type = types.addCheck types.str (x: builtins.trace "devenv path: '${x}'" true);
      description = "A tracked string path";
    };
  };
}
