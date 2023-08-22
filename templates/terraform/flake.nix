{
  inputs = {
    devenv.url = "github:cachix/devenv";
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/nixos-23.05";
  };

  outputs = { self, devenv, flake-utils, nixpkgs }@inputs:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = devenv.lib.mkShell {
          inherit inputs pkgs;
          modules = [{
            env.AWS_PROFILE = "<profile>";

            languages.terraform.enable = true;

            pre-commit.hooks.terraform-format.enable = true;
          }];
        };
      });
}
