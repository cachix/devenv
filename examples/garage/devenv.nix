{ ... }:

{
  services.garage = {
    enable = true;
    rpcSecret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    buckets = [ "test-bucket" ];
  };
}
