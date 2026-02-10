{ pkgs, ... }:
{
  # Sandbox is enabled via devenv.yaml (cannot be set here - it's readOnly)

  packages = [
    pkgs.coreutils
  ];
}
