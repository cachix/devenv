{ config, lib, pkgs, ... }:

{
  packages = [ pkgs.curl pkgs.jq ];

  scripts.silly-example.exec = ''curl "https://httpbin.org/get?$1" | jq .args'';
  scripts.silly-example.description = "curls httpbin with provided arg";

  scripts.serious-example.exec = ''${pkgs.cowsay}/bin/cowsay "$*"'';
  scripts.serious-example.description = ''echoes args in a very serious manner'';

  enterShell = ''
    echo
    echo 🦾 Helper scripts you can run to make your development richer:
    echo 🦾
    ${pkgs.gnused}/bin/sed -e 's| |••|g' -e 's|=| |' <<EOF | ${pkgs.util-linuxMinimal}/bin/column -t | ${pkgs.gnused}/bin/sed -e 's|^|🦾 |' -e 's|••| |g'
    ${lib.generators.toKeyValue {} (lib.mapAttrs (name: value: value.description) config.scripts)}
    EOF
    echo
  '';
}
