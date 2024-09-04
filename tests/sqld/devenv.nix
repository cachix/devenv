{ pkgs, ... }:
{
  packages = with pkgs; [ turso-cli ];

  services.sqld = {
    enable = true;
    port = 6000;
  };

  scripts.sqld-check.exec = ''
    $DEVENV_PROFILE/bin/turso db shell http://127.0.0.1:6000 ".schema"
  '';
}
