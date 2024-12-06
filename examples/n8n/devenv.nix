{ pkgs, ... }:

{
  packages = [ pkgs.coreutils ];
  services.n8n = {
    enable = true;
    address = "0.0.0.0";
    port = 5432;
    # settings = {

    # }
    # webhookUrl = "";
  };
}
