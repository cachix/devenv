args@{
  system,
  # The project root (location of devenv.nix)
  devenv_root,
  devenv_lock,
  ...
}:

let
  inherit
    (import ./resolve-lock.nix {
      src = devenv_root;
      lockFilePath = devenv_lock;
      inherit system;
    })
    inputs
    ;

  bootstrapLib = import ./bootstrapLib.nix { inherit inputs; };
in

bootstrapLib.mkDevenvForSystem args
