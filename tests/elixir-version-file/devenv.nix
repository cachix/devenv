{
  languages.elixir = {
    enable = true;
    lsp.enable = false;
    versionFile = ./.tool-versions;
  };

  enterTest = ''
    # The -otp-26 suffix is ignored. Elixir 1.17 supports OTP up to 27, so OTP 27 is picked.
    test "$(elixir -e 'IO.write(System.version())')" = "1.17.3"
    test "$(erl -noshell -eval 'io:format("~s", [erlang:system_info(otp_release)]), halt().')" = "27"
  '';
}
