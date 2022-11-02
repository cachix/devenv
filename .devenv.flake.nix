{
  inputs = (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs;

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
          /nix/store/kz6sxf9k39mxd1ay581vj7dfnkdiwyy6-modules/top-level.nix
          ./devenv.nix
          (devenv.devenv or {})
        ] ++ (map toModule (devenv.imports or []));
      };
      config = project.config;
    in {
      packages."x86_64-linux" = {
        build = config.build;
        procfile = config.procfile;
        procfileEnv = config.procfileEnv;
      };
      devShell."x86_64-linux" = config.shell;
    };
}
