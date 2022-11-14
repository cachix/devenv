{ pkgs }: pkgs.writeText "devenv-flake" ''
  {
    inputs = { 
      pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
      pre-commit-hooks.inputs.nixpkgs.follows = "nixpkgs";
      nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
      devenv.url = "github:cachix/devenv";
      devenv.inputs.nixpkgs.follows = "nixpkgs";
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
          else let 
            paths = lib.splitString "/" path;
            name = builtins.head paths;
            input = inputs.''${name} or (throw "Unknown input ''${name}");
            subpath = "/''${lib.concatStringsSep "/" (builtins.tail paths)}";
            devenvpath = input + subpath + "/devenv.nix";
            in if builtins.pathExists devenvpath
               then devenvpath
               else {};
        project = pkgs.lib.evalModules {
          specialArgs = inputs // { inherit pkgs; };
          modules = [
            (inputs.devenv.modules + /top-level.nix)
            ./devenv.nix
            (devenv.devenv or {})
            (if builtins.pathExists ./devenv.local.nix then ./devenv.local.nix else {})
          ] ++ (map toModule (devenv.imports or []));
        };
        config = project.config;
      in {
        packages."${pkgs.system}" = {
          ci = pkgs.runCommand "ci" {} ("ls " + toString config.ci + " && touch $out");
          procfile = config.procfile;
          procfileEnv = config.procfileEnv;
        };
        devShell."${pkgs.system}" = config.shell;
      };
  }
''
