{
  languages.zig = {
    enable = true;
    version = "0.16.0";
  };

  enterTest = ''
    test "$(zig version)" = "0.16.0"
    test "$(zls --version)" = "0.16.0"
  '';
}
