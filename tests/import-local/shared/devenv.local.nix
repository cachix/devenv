{ lib, ... }:
{
  # This should override SHARED_VAR from devenv.nix
  env.SHARED_VAR = lib.mkForce "from_local";
  # Unique var to verify devenv.local.nix is loaded
  env.LOCAL_ONLY = "yes";
}
