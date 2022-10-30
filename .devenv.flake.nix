{
  inputs = (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs;

  outputs = { nixpkgs, ... }@inputs: 
    let
      pkgs = import nixpkgs { system = "x86_64-linux"; };
      project = pkgs.lib.evalModules {
        specialArgs = inputs // { inherit pkgs; };
        modules = [
          /nix/store/592ybaphq47djh3ddxnw8scyh07a4d6z-modules/top-level.nix
          # TODO: how to improve errors here coming from this file?
          # TODO: this won't work for packages :(
          ./devenv.nix
          ((builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).devenv or {})
        ];
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
