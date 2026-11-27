{ pkgs, ... }:
{
  languages.ruby = {
    enable = true;
    version = "3.4.7";
    documentation.enable = true;
    # Solargraph is tested separately in the Ruby example.
    lsp.enable = false;
  };

  enterTest = ''
    ri Object >/dev/null
  '';

}
