{ pkgs, ... }:

{

  services.minio.enable = true;
  services.minio.accessKey = "minioadmin";
  services.minio.secretKey = "minioadmin";
  services.minio.buckets = [ "testbucket" ];

}
