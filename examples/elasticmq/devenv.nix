{ pkgs, ... }:

{
  services.elasticmq.enable = true;
  services.elasticmq.settings = ''
    queues {
      test-queue {}
    }
  '';
}
