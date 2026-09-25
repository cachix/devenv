{
  languages.zig = {
    enable = true;
    version = "0.16.0";
    lsp.enable = false;
  };

  enterTest = ''
    test "$(zig version)" = "0.16.0"
  '';
}
