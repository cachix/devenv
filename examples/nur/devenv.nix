{ pkgs, inputs, config, ... }:

{
  imports = [ inputs.nur.nixosModules.nur ];

  # see the list of repos at https://nur.nix-community.org/documentation/
  packages = [
    config.nur.repos.mic92.hello-nur
  ];
}
