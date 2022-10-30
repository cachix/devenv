{ pkgs }: pkgs.writeText "devenv-flake" ''
{
  inputs = (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs;

  outputs = { nixpkgs, ... }@inputs: 
    let
      pkgs = import nixpkgs { system = "${pkgs.system}"; };
      project = pkgs.lib.evalModules {
        specialArgs = inputs // { inherit pkgs; };
        modules = [
          ${./modules}/top-level.nix
          # TODO: how to improve errors here coming from this file?
          # TODO: this won't work for packages :(
          ./devenv.nix
          ((builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).devenv or {})
        ];
      };
      config = project.config;
    in {
      packages."${pkgs.system}" = {
        build = config.build;
        procfile = config.procfile;
        procfileEnv = config.procfileEnv;
      };
      includes = config.includes;
      devShell."${pkgs.system}" = config.shell;
    };
}
''