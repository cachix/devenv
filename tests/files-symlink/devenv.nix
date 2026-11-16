{ ... }:
let
  version = import ./version.nix;
in
{
  files = {
    "linked directory".source = if version == 1 then ./source-one else ./source-two;
    "linked file".text = "version ${toString version}";
    "conflicting file".text = "generated";
    "conflicting directory".text = "generated";
  };
}
