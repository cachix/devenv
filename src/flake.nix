{ pkgs, version }: pkgs.writeText "devenv-flake" ''
  {
    inputs = {
      pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
      pre-commit-hooks.inputs.nixpkgs.follows = "nixpkgs";
      nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
      devenv.url = "github:cachix/devenv?dir=src/modules";
    } // (if builtins.pathExists ./.devenv/devenv.json 
         then (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs
         else {});

    outputs = { nixpkgs, ... }@inputs:
      let
        pkgs = import nixpkgs { system = "${pkgs.system}"; };
        lib = pkgs.lib;
        devenv = if builtins.pathExists ./.devenv/devenv.json
          then builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)
          else {};
        toModule = path:
          if lib.hasPrefix "./" path
          then ./. + (builtins.substring 1 255 path) + "/devenv.nix"
          else if lib.hasPrefix "../" path 
          then throw "devenv: ../ is not supported for imports"
          else let
            paths = lib.splitString "/" path;
            name = builtins.head paths;
            input = inputs.''${name} or (throw "Unknown input ''${name}");
            subpath = "/''${lib.concatStringsSep "/" (builtins.tail paths)}";
            devenvpath = "''${input}/" + subpath + "/devenv.nix";
            in if (!devenv.inputs.''${name}.flake or true) && builtins.pathExists devenvpath
               then devenvpath
               else throw (devenvpath + " file does not exist for input ''${name}.");
        project = pkgs.lib.evalModules {
          specialArgs = inputs // { inherit inputs pkgs; };
          modules = [
            (inputs.devenv.modules + /top-level.nix)
            { devenv.cliVersion = "${version}"; }
          ] ++ (map toModule (devenv.imports or [])) ++ [
            ./devenv.nix
            (devenv.devenv or {})
            (if builtins.pathExists ./devenv.local.nix then ./devenv.local.nix else {})
          ];
        };
        config = project.config;
      in {
        packages."${pkgs.system}" = {
          ci = pkgs.runCommand "ci" {} ("ls " + toString config.ci + " && touch $out");
          inherit (config) info procfileScript procfileEnv procfile;
        };
        devShell."${pkgs.system}" = config.shell;
      };
  }
''
