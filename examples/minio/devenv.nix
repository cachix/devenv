{ pkgs, ... }:

{

  services.minio.enable = true;
  services.minio.buckets = [ "testbucket" ];

}
