{ ... }:
{
  services.opensearch.enable = true;

  enterTest = ''
    wait_for_processes
  '';
}
