{ pkgs, ... }:
{
  languages.ruby = {
    enable = true;
    version = "3.4.7";
    documentation.enable = true;
    # solargraph pulls nokogiri 1.18.10 which fails to build on aarch64-darwin
    lsp.enable = false;
  };

  enterTest = ''
    ri Object >/dev/null
  '';

}
