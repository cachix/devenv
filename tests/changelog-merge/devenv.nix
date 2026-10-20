{ pkgs, ... }:
{
  changelog-merge.enable = true;
  packages = [ pkgs.git ];

  # The integration must append its configuration without losing this entry.
  env = {
    GIT_CONFIG_COUNT = "1";
    GIT_CONFIG_KEY_0 = "test.preserved";
    GIT_CONFIG_VALUE_0 = "existing value";
  };
}
