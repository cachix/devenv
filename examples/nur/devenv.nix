{ pkgs, inputs, config, ... }:

{
  # see the list of repos at https://nur.nix-community.org/documentation/
  packages = [
    pkgs.nur.repos.mic92.hello-nur
  ];
}
