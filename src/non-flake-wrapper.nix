mkShell: projectRoot:
{
  pkgs,
  inputs ? { nixpkgs = pkgs; },
  modules,
}:
mkShell {
  inputs = {
    self = projectRoot;
  }
  // inputs;
  inherit pkgs modules;
}
