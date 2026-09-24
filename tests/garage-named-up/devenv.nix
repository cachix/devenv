{ ... }:
{
  services.garage = {
    enable = true;
    rpcSecret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    buckets = [ "named-up-bucket" ];
  };
}
