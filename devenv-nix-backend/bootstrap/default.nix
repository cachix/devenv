args@{ system
, # The project root (location of devenv.nix)
  devenv_root
, ...
}:

let
  inherit
    (import ./resolve-lock.nix {
      src = devenv_root;
      inherit system;
    })
    inputs
    ;

  bootstrapLib = import ./bootstrapLib.nix { inherit inputs; };
in

bootstrapLib.mkDevenvForSystem args
