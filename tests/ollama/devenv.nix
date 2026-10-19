{ pkgs, ... }:

{
  packages = [ pkgs.curl ];

  services.ollama.enable = true;
}
