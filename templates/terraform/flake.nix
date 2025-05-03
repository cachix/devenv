{
  inputs = {
    devenv.inputs.nixpkgs.follows = "nixpkgs";
    devenv.url = "github:cachix/devenv";
    nixpkgs-terraform.url = "github:stackbuilders/nixpkgs-terraform";
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    systems.url = "github:nix-systems/default";
  };

  nixConfig = {
    extra-substituters = "https://devenv.cachix.org https://nixpkgs-terraform.cachix.org";
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw= nixpkgs-terraform.cachix.org-1:8Sit092rIdAVENA3ZVeH9hzSiqI/jng6JiCrQ1Dmusw=";
  };

  outputs = { self, devenv, nixpkgs, systems, ... }@inputs:
    let
      forEachSystem = nixpkgs.lib.genAttrs (import systems);
    in
    {
      devShells = forEachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = devenv.lib.mkShell {
            inherit inputs pkgs;
            modules = [{
              languages.terraform.enable = true;
              languages.terraform.version = "1.8.4";

              git-hooks.hooks.terraform-format.enable = true;
            }];
          };
        });
    };
}
