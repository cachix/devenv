{
  description = "devenv.sh - Fast, Declarative, Reproducible, and Composable Developer Environments";

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw= cachix.cachix.org-1:eWNHQldwUO7G2VkjpnjDbWwy4KQ/HNxht7H4SSoMckM=";
    extra-substituters = "https://devenv.cachix.org https://cachix.cachix.org";
  };

  # this needs to be rolling so we're testing what most devs are using
  inputs.nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
  inputs.git-hooks = {
    url = "github:cachix/git-hooks.nix";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
    };
  };
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };
  inputs.flake-parts = {
    url = "github:hercules-ci/flake-parts";
    inputs = {
      nixpkgs-lib.follows = "nixpkgs";
    };
  };
  inputs.nix = {
    url = "github:cachix/nix/devenv-2.30.6";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
      flake-parts.follows = "flake-parts";
      git-hooks-nix.follows = "git-hooks";
      nixpkgs-23-11.follows = "";
      nixpkgs-regression.follows = "";
    };
  };
  inputs.cachix = {
    url = "github:cachix/cachix/latest";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
      git-hooks.follows = "git-hooks";
      devenv.follows = "";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      git-hooks,
      nix,
      ...
    }@inputs:
    let
      systems = [
        "x86_64-linux"
        "i686-linux"
        "x86_64-darwin"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          overlays = [
            (final: prev: {
              inherit (inputs.cachix.packages.${system}) cachix;
              devenv-nix = inputs.nix.packages.${system}.nix-cli;
            })
          ];
          pkgs = import nixpkgs { inherit overlays system; };
          workspace = pkgs.callPackage ./workspace.nix { };
        in
        {
          inherit (workspace.crates) devenv devenv-tasks devenv-tasks-fast-build;
          default = self.packages.${system}.devenv;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          devenv-image = import ./containers/devenv/image.nix {
            inherit pkgs;
            inherit (self.packages.${system}) devenv;
            nixInput = inputs.nix;
          };
        }
      );

      modules = ./src/modules;

      templates =
        let

          flake-parts = {
            path = ./templates/flake-parts;
            description = "A flake with flake-parts, direnv and devenv.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };

          simple = {
            path = ./templates/simple;
            description = "A direnv supported Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };
        in
        {
          inherit simple flake-parts;
          terraform = {
            path = ./templates/terraform;
            description = "A Terraform Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };
          default = simple;
        };

      flakeModule = self.flakeModules.default; # Backwards compatibility
      flakeModules = {
        default = import ./flake-module.nix self;
        readDevenvRoot =
          { inputs, lib, ... }:
          {
            config =
              let
                devenvRootFileContent =
                  if inputs ? devenv-root then builtins.readFile inputs.devenv-root.outPath else "";
              in
              lib.mkIf (devenvRootFileContent != "") {
                devenv.root = devenvRootFileContent;
              };
          };
      };

      lib = {
        mkConfig =
          args@{
            pkgs,
            inputs,
            modules,
          }:
          (self.lib.mkEval args).config;
        mkEval =
          {
            pkgs,
            inputs,
            modules,
          }:
          let
            moduleInputs = {
              inherit git-hooks;
            }
            // inputs;
            project = inputs.nixpkgs.lib.evalModules {
              specialArgs = moduleInputs // {
                inputs = moduleInputs;
              };
              modules = [
                { config._module.args.pkgs = inputs.nixpkgs.lib.mkDefault pkgs; }
                (self.modules + /top-level.nix)
                (
                  { config, ... }:
                  {
                    devenv.warnOnNewVersion = false;
                    devenv.flakesIntegration = true;
                  }
                )
              ]
              ++ modules;
            };
          in
          project;
        mkShell =
          args:
          let
            config = self.lib.mkConfig args;
          in
          config.shell
          // {
            ci = config.ciDerivation;
            inherit config;
          };
          nonFlakeMkShell = import ./src/non-flake-wrapper.nix self.lib.mkShell;
      };

      overlays.default = final: prev: {
        devenv = self.packages.${prev.system}.default;
      };
    };
}
