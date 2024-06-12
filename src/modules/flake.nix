{
  outputs = { ... }: {
    modules = ./.;
    isTmpDir = true;
    hasIsTesting = true;
  };
}
