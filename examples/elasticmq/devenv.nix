{ pkgs, ... }:

{
  packages = [
    pkgs.jq
  ];

  services.elasticmq.enable = true;
  services.elasticmq.settings = ''
    queues {
      test-queue {}
    }
  '';
}
