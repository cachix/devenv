{ inputs
, pkgs
, flakesIntegration ? false
}:

let
  input = inputs."nixpkgs-multiverse" or (throw (
    if flakesIntegration then
      ''
        To use `multiverse`, add the following input to your flake:

          inputs.nixpkgs-multiverse.url = "github:fzakaria/nixpkgs-multiverse";
      ''
    else
      ''
        To use `multiverse`, run the following command:

          $ devenv inputs add nixpkgs-multiverse github:fzakaria/nixpkgs-multiverse
      ''
  ));

  mkMultiverse = input.lib.mkMultiverse or (throw ''
    The `nixpkgs-multiverse` input does not provide `lib.mkMultiverse`.
    Expected an input compatible with github:fzakaria/nixpkgs-multiverse.
  '');

  # Some nixpkgs configuration hooks default to null in newer revisions, while
  # older revisions only check whether the attribute exists before calling it.
  nixpkgsConfig = pkgs.lib.filterAttrs (_: value: value != null) pkgs.config;

  multiverse = mkMultiverse {
    system = pkgs.stdenv.hostPlatform.system;
    config = nixpkgsConfig;
  };

  updateCommand =
    if flakesIntegration then
      "nix flake update nixpkgs-multiverse"
    else
      "devenv update nixpkgs-multiverse";

  # Revision minimizing arrived after the first indexed releases, so an input
  # locked before it serves versions but cannot plan across them.
  solvePins = multiverse.solvePins or (throw ''
    `multiverse.pins` needs a `nixpkgs-multiverse` input that resolves pins
    through the fewest nixpkgs revisions. Update the input:

      $ ${updateCommand}
  '');

  # The cost of a multiverse is per revision touched, not per package: every
  # extra revision is another nixpkgs to fetch and evaluate. `pins` resolves a
  # whole set of versions through the fewest revisions that can serve them and
  # hands back the packages themselves, ready for `packages`. `pins` is not a
  # nixpkgs attribute, so the version index stays unshadowed.
  pins = requested: builtins.attrValues (solvePins requested);
in
multiverse.versions // { inherit pins; }
