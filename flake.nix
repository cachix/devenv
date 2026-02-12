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
    url = "github:cachix/nix/devenv-2.32";
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
  inputs.nixd = {
    url = "github:nix-community/nixd";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-parts.follows = "flake-parts";
    };
  };
  inputs.crate2nix = {
    url = "github:nix-community/crate2nix";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    { self
    , nixpkgs
    , git-hooks
    , nix
    , ...
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
            inputs.rust-overlay.overlays.default
            (final: prev: {
              inherit (inputs.cachix.packages.${system}) cachix;
              nix = inputs.nix.packages.${system}.nix;
              nixd = inputs.nixd.packages.${system}.nixd;
              crate2nix = inputs.crate2nix.packages.${system}.default;
            })
          ];
          pkgs = import nixpkgs { inherit overlays system; };
          gitRev = self.shortRev or (self.dirtyShortRev or "");
          # Use stable Rust from rust-overlay for crate2nix builds
          # (nixpkgs' buildRustCrate uses Rust 1.73 which is too old for some deps)
          rustToolchain = pkgs.rust-bin.stable.latest.default;
          workspace = pkgs.callPackage ./workspace.nix {
            inherit pkgs gitRev;
            rustc = rustToolchain;
            cargo = rustToolchain;
          };
        in
        {
          inherit (workspace.crates) devenv devenv-tasks devenv-tasks-fast-build;
          default = self.packages.${system}.devenv;
          crate2nix = inputs.crate2nix.packages.${system}.default;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          devenv-image = import ./containers/devenv/image.nix {
            inherit pkgs;
            inherit (self.packages.${system}) devenv;
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

          flake = {
            path = ./templates/flake;
            description = "A direnv supported Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };

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
        in
        {
          inherit flake flake-parts terraform;
          simple = flake; # Backwards compatibility
          default = self.templates.flake;
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
        mkConfig = args: (self.lib.mkEval args).config;

        mkEval =
          args@{ pkgs
          , inputs
          , modules
          , lib ? pkgs.lib
          ,
          }:
          let
            # TODO: deprecate default git-hooks input
            defaultInputs = { inherit git-hooks; };
            finalInputs = defaultInputs // inputs;

            specialArgs = finalInputs // {
              inputs = finalInputs;
            };

            modules = [
              (self.modules + /top-level.nix)
              (
                { config, ... }:
                {
                  # Configure overlays
                  _module.args.pkgs = pkgs.appendOverlays config.overlays;
                  # Enable the flakes integration
                  devenv.flakesIntegration = true;
                  # Disable CLI version checks
                  devenv.warnOnNewVersion = false;
                }
              )
            ]
            ++ args.modules;

            project = lib.evalModules { inherit modules specialArgs; };
          in
          project;

        mkShell =
          args:
          let
            config = self.lib.mkConfig args;
          in
          config.shell
          // {
            inherit config;
            ci = config.ciDerivation;
          };
      };

      overlays.default = final: prev: {
        devenv = self.packages.${prev.system}.default;
      };
    };
}
