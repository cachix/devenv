{
  languages.elixir = {
    enable = true;
    lsp.enable = false;
    # This directory doesn't exist yet, like on a fresh checkout.
    directory = "./backend";
    mix.deps.enable = true;
  };

  # Reaching this point means entering the shell worked.
  enterTest = ''
    test ! -e backend
    devenv tasks run devenv:elixir:deps
  '';
}
