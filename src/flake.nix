{ pkgs }: pkgs.writeText "devenv-flake" ''
  {
    inputs = { pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
      } // (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs;
    
    outputs = { nixpkgs, ... }@inputs:
      let
        pkgs = import nixpkgs { system = "${pkgs.system}"; };
        lib = pkgs.lib;
        devenv = builtins.fromJSON (builtins.readFile ./.devenv/devenv.json);
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
            ${./modules}/top-level.nix
            ./devenv.nix
            (devenv.devenv or {})
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
