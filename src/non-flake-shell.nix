self:
{
  pkgs,
  inputs ? {
    nixpkgs = pkgs;
  },
  modules,
}:
self.lib.mkShell {
  inputs = {
    self = self;
  }
  // inputs;
  inherit pkgs modules;
}
