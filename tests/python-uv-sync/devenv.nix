{ pkgs, config, ... }:
: let
pkgs-unstable = import inputs.nixpkgs-unstable { system = pkgs.stdenv.system; };
in {
languages.python = {
enable = true;
directory = "./directory";
uv = {
enable = true;
package = pkgs-unstable.uv;
sync.enable = true;
};
};
}
