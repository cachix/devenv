{ system }: ''

{
  inputs = (builtins.fromJSON (builtins.readFile dev.json)).inputs;

  outputs = { nixpkgs }: 
    let
      pkgs = import nixpkgs { system = "${system}"; };
      project = (lib.evalModules {
        specialArgs = { };
        modules = [ 
          ${./module.nix} 
          # TODO: how to improve errors here coming from this file?
          # TODO: this won't work for packages :(
          ((builtins.fromJSON (builtins.readFile dev.json)).devenv or {})
        ];
      }).config;
    in
    {
      packages = {
        build = project.build;
        procfile project.procfile;
      };
      devShell = project.shell;
    }
}
''