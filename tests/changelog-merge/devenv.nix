{ pkgs, ... }:
{
  changelog-merge.enable = true;
  packages = [ pkgs.git ];

  # The driver must compose with other Git configuration and attributes.
  git.config.test.preserved = "existing value";
  git.attributes = [ "*.txt text" ];
}
