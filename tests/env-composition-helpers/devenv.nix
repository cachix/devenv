{ inputs, pkgs, ... }:
{
  packages = [
    pkgs.nix
    pkgs.python3
    pkgs.bash
  ];

  enterTest = ''
    python3 ${./.}/check.py \
      --nixpkgs ${pkgs.path} \
      --library ${inputs.devenv.modules}/lib/env.nix
  '';
}
