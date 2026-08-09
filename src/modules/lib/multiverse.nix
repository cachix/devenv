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
in
(mkMultiverse {
  system = pkgs.stdenv.hostPlatform.system;
  config = nixpkgsConfig;
}).versions
