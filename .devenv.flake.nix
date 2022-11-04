{
  inputs = { pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
    } // (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs;
  

  outputs = { nixpkgs, ... }@inputs:
    let
      pkgs = import nixpkgs { system = "x86_64-linux"; };
      lib = pkgs.lib;
      devenv = builtins.fromJSON (builtins.readFile ./.devenv/devenv.json);
      toModule = path: 
        if lib.hasPrefix "./" path 
        then ./. + (builtins.substring 1 255 path) + "/devenv.nix"
        else let 
          paths = lib.splitString "/" path;
          first = builtins.head paths;
          in inputs.${first} or (throw "Unknown input ${first}") + "/${lib.concatStringsSep "/" (builtins.tail paths)}" + "/devenv.nix";
      project = pkgs.lib.evalModules {
        specialArgs = inputs // { inherit pkgs; };
        modules = [
          /nix/store/66h0a8yh05k9ilvzf22hp47zr3r8gwbs-modules/top-level.nix
          ./devenv.nix
          (devenv.devenv or {})
        ] ++ (map toModule (devenv.imports or []));
      };
      config = project.config;
    in {
      packages."x86_64-linux" = {
        ci = pkgs.runCommand "ci" {} ("ls " + toString config.ci + " && touch $out");
        procfile = config.procfile;
        procfileEnv = config.procfileEnv;
      };
      devShell."x86_64-linux" = config.shell;
    };
}
