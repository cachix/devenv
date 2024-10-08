{ pkgs, config, inputs, ... }:
let
  pkgs-unstable = import inputs.nixpkgs-unstable { system = pkgs.stdenv.system; };
in
{
  languages.python = {
    enable = true;
    directory = "./directory";
    venv.enable = true;
    uv = {
      enable = true;
      package = pkgs-unstable.uv;
      sync.enable = true;
    };
  };
}
