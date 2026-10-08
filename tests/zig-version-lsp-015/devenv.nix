{
  languages.zig = {
    enable = true;
    version = "0.15.1";
  };

  enterTest = ''
    test "$(zig version)" = "0.15.1"
    test "$(zls --version)" = "0.15.0"
  '';
}
