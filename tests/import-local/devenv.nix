{ config, ... }:
{
  env.MAIN_CONFIG = "true";

  assertions = [
    {
      assertion = config.env.SHARED_VAR or "" != "";
      message = "SHARED_VAR is not set. The ./shared/devenv.nix was not loaded correctly.";
    }
    {
      assertion = config.env.SHARED_BASE or "" != "";
      message = "SHARED_BASE is not set. The ./shared/devenv.nix was not loaded correctly.";
    }
    {
      assertion = config.env.LOCAL_ONLY or "" == "yes";
      message = "LOCAL_ONLY should be 'yes' (set by devenv.local.nix), but got: '${config.env.LOCAL_ONLY or ""}'";
    }
    {
      assertion = config.env.SHARED_VAR or "" == "from_local";
      message = "SHARED_VAR should be 'from_local' (overridden by devenv.local.nix with mkForce), but got: '${config.env.SHARED_VAR or ""}'";
    }
  ];
}
