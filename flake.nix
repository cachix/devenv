{
  description = "devenv - Developer Environments";

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.pre-commit-hooks = {
    url = "github:cachix/pre-commit-hooks.nix";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
    };
  };
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };
  inputs.nix = {
    url = "github:domenkozar/nix/relaxed-flakes";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, pre-commit-hooks, nix, ... }:
    let
      systems = [ "x86_64-linux" "i686-linux" "x86_64-darwin" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = f: builtins.listToAttrs (map (name: { inherit name; value = f name; }) systems);
      mkPackage = pkgs: import ./src/devenv.nix { inherit pkgs nix; };
      mkDocOptions = pkgs:
        let
          eval = pkgs.lib.evalModules {
            modules = [ ./src/modules/top-level.nix ];
            specialArgs = { inherit pre-commit-hooks pkgs; };
          };
          options = pkgs.nixosOptionsDoc {
            options = builtins.removeAttrs eval.options [ "_module" ];
          };
        in
        options.optionsCommonMark;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          devenv = mkPackage pkgs;
          devenv-docs-options = mkDocOptions pkgs;
        }
      );

      modules = ./src/modules;

      defaultPackage = forAllSystems (system: self.packages.${system}.devenv);

      templates.simple = {
        path = ./templates/simple;
        description = "A direnv supported Nix flake with devenv integration.";
      };

      lib = {
        mkConfig = { pkgs, inputs, modules }:
          let
            moduleInputs = { inherit pre-commit-hooks; } // inputs;
            project = inputs.nixpkgs.lib.evalModules {
              specialArgs = moduleInputs // {
                inherit pkgs;
                inputs = moduleInputs;
              };
              modules = [
                (self.modules + /top-level.nix)
              ] ++ modules;
            };
          in
          project.config;
        mkShell = args: (self.lib.mkConfig args).shell;
      };
    };
}
