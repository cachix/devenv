{ pkgs, lib, config, ... }: {
  # This should fail: trying to override a readOnly option
  devenv.sandbox.enable = lib.mkForce false;

  packages = [ pkgs.hello ];
}
