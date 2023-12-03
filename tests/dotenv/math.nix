{ nixpkgs }:
let
  inherit (nixpkgs.lib) mod;
in
{
  # Returns true if integer is even.
  isEven = x: (mod x 2) == 0;
}
