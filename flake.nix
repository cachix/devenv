{
  description = "devenv - Developer Environments";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-22.05";

  outputs = { self, nixpkgs, ... }: 
    let
      systems = [ "x86_64-linux" "i686-linux" "x86_64-darwin" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = f: builtins.listToAttrs (map (name: { inherit name; value = f name; }) systems);
    in
      {
        packages = forAllSystems (
          system: {
            devenv = let 
              pkgs = (import nixpkgs { inherit system; });
            in pkgs.resholve.writeScriptBin "devenv" {
              inputs = [ pkgs.foreman pkgs.yaml2json ];
              interpreter = "${pkgs.bash}/bin/bash";
            } (import ./src/devenv.nix { inherit system; });
                }
        );

        defaultPackage = forAllSystems (system: self.packages.${system}.envenv);
      };
}