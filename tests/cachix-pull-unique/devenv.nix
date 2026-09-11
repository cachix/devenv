{ ... }:
{
  # "devenv" is also added by the cachix module itself, and "nixpkgs-python"
  # is listed twice on purpose: cachix.pull must come out without duplicates.
  cachix.pull = [
    "devenv"
    "nixpkgs-python"
    "nixpkgs-python"
  ];
}
