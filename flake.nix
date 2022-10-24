{
  description = "devenv - Developer Environments";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-22.05";

  outputs = { self, nixpkgs, ... }: 
    let
      systems = [ "x86_64-linux" "i686-linux" "x86_64-darwin" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = f: builtins.listToAttrs (map (name: { inherit name; value = f name; }) systems);
      mkPackage = pkgs: import ./src/devenv.nix { inherit pkgs; };
      mkDocOptions = pkgs:
       let
          eval = pkgs.lib.evalModules {
            modules = [./src/module.nix ];
          };
          options = pkgs.nixosOptionsDoc {
            options = eval.options;
          };

        in options.optionsCommonMark;
    in
      {
        packages = forAllSystems (system: 
          let 
            pkgs = (import nixpkgs { inherit system; });
          in {
            devenv = mkPackage pkgs;
            devenv-docs-options = mkDocOptions pkgs;
          }            
        );

        defaultPackage = forAllSystems (system: self.packages.${system}.devenv);
      };
}

