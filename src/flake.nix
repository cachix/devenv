{ pkgs }: pkgs.writeText "devenv-flake" ''
{
  inputs = (builtins.fromJSON (builtins.readFile ./devenv.json)).inputs;

  outputs = { nixpkgs, ... }@inputs: 
    let
      pkgs = import nixpkgs { system = "${pkgs.system}"; };
      project = (pkgs.lib.evalModules {
        specialArgs = inputs // { inherit pkgs; };
        modules = [ 
          ${./module.nix} 
          # TODO: how to improve errors here coming from this file?
          # TODO: this won't work for packages :(
          ((builtins.fromJSON (builtins.readFile ./devenv.json)).devenv or {})
        ];
      }).config;
    in {
      packages."${pkgs.system}" = {
        build = project.build;
        procfile = project.procfile;
      };
      devShell."${pkgs.system}" = project.shell;
    };
}
''