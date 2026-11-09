{
  languages.elixir = {
    enable = true;
    version = "1.17.3";
  };

  enterTest = ''
    for tool in elixir erl erlc escript elixir-ls; do
      command -v "$tool"
    done

    test "$(elixir -e 'IO.write(System.version())')" = "1.17.3"

    # Elixir 1.17 supports OTP up to 27, so the overlay selects OTP 27.
    test "$(erl -noshell -eval 'io:format("~s", [erlang:system_info(otp_release)]), halt().')" = "27"

    # Compare the actual runtime, including its store path, rather than just the OTP major.
    elixir_runtime=$(elixir -e 'IO.write(:erlang.system_info(:version)); IO.write(" "); IO.write(:code.root_dir())')
    erlang_runtime=$(erl -noshell -eval 'io:format("~s ~s", [erlang:system_info(version), code:root_dir()]), halt().')
    test "$elixir_runtime" = "$erlang_runtime"
  '';
}
