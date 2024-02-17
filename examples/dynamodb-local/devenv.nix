{ pkgs, ... }:

{
  services.dynamodb-local.enable = true;
  packages = [
    pkgs.awscli2
  ];
}
