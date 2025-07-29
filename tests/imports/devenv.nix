{ config, inputs, ... }:
{
  env.MAIN_CONFIG = "true";

  assertions = [
    {
      assertion = config.env.SUBDIR1_VAR or "" != "";
      message = "SUBDIR1_VAR is not set. The ./subdir1/devenv.nix was not loaded correctly.";
    }
    {
      assertion = config.env.SUBDIR2_VAR or "" != "";
      message = "SUBDIR2_VAR is not set. The ./subdir2/devenv.nix was not loaded correctly.";
    }
    {
      assertion = config.env.SUBDIR3_VAR or "" != "";
      message = "SUBDIR3_VAR is not set. The ./subdir3/devenv.nix was not loaded correctly.";
    }
    {
      assertion = config.env.MAIN_CONFIG or "" != "";
      message = "MAIN_CONFIG is not set. The main devenv.nix was not loaded correctly.";
    }
    {
      assertion = builtins.hasAttr "flake-utils" inputs;
      message = "inputs.flake-utils does not exist. The ./subdir1/devenv.yaml input was not imported correctly.";
    }
  ];
}
