{ pkgs, lib, config, inputs, ... }: {
  dotenv.enable = true;

  env.BAR = "1";

  # Assert that flake-utils input exists from devenv.local.yaml
  assertions = [
    {
      assertion = inputs ? flake-utils;
      message = "flake-utils input should be available from devenv.local.yaml";
    }
  ];
}
