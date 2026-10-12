{ pkgs, ... }:
{
  languages.elixir.enable = true;
  languages.elixir.lsp.enable = false;

  languages.elixir = {
    # Model a custom package that does not expose its runtime dependency.
    package = pkgs.beamPackages.elixir // {
      buildInputs = [ ];
    };
    erlang.package = pkgs.beamPackages.erlang;
  };

  enterTest = ''
    for tool in elixir erl erlc escript; do
      command -v "$tool"
    done

    # Compare the actual runtime, including its store path, rather than just the OTP major.
    elixir_runtime=$(elixir -e 'IO.write(:erlang.system_info(:version)); IO.write(" "); IO.write(:code.root_dir())')
    erlang_runtime=$(erl -noshell -eval 'io:format("~s ~s", [erlang:system_info(version), code:root_dir()]), halt().')
    test "$elixir_runtime" = "$erlang_runtime"

    cat > runtime_probe.erl <<'ERLANG'
    -module(runtime_probe).
    -export([main/1]).
    main(_) -> io:format("compiled and executed~n").
    ERLANG
    erlc runtime_probe.erl
    test "$(escript runtime_probe.beam)" = "compiled and executed"
  '';
}
