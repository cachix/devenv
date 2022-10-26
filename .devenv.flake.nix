{
  inputs = (builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).inputs;

  outputs = { nixpkgs, ... }@inputs: 
    let
      pkgs = import nixpkgs { system = "x86_64-linux"; };
      project = (pkgs.lib.evalModules {
        specialArgs = inputs // { inherit pkgs; };
        modules = [ 
          /nix/store/xhx6wpxg9n3qk80qzhhrif61vsvy9ibi-module.nix 
          # TODO: how to improve errors here coming from this file?
          # TODO: this won't work for packages :(
          ((builtins.fromJSON (builtins.readFile ./.devenv/devenv.json)).devenv or {})
        ];
      }).config;
    in {
      packages."x86_64-linux" = {
        build = project.build;
        procfile = project.procfile;
      };
      devShell."x86_64-linux" = project.shell;
    };
}
