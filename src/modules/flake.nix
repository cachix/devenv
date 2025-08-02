{
  outputs =
    { ... }:
    {
      modules = ./.;
      isTmpDir = builtins.warn "`inputs.devenv.isTmpDir` is deprecated. Use `config.devenv.tmpdir` directly instead." true;
      hasIsTesting = builtins.warn "`inputs.devenv.hasIsTesting` is deprecated. Use `config.devenv.isTesting` directly instead." true;
    };
}
