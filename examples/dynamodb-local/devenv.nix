{ pkgs, config, ... }:

{
  services.dynamodb-local = {
    enable = true;
    sharedDb = true;
  };
  packages = [
    pkgs.awscli2
  ];

  enterTest = ''
    export DYNAMODB_PORT=${toString config.processes.dynamodb.ports.main.value}
  '';
}
