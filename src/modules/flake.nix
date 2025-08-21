{
  outputs =
    { ... }:
    {
      modules = ./.;

      # Legacy feature-detection flags
      # These are used by older devenv CLIs to detect certain module features that require template inputs.
      # Deprecated as of 1.8.2
      isTmpDir = true;
      hasIsTesting = true;
    };
}
