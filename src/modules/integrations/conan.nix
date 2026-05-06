{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.conan;

  inputArgs = {
    name = "conan-flake";
    url = "git+https://codeberg.org/tarcisio/conan-flake";
    attribute = "conan";
  };

  # When enabled, use getInput (throws helpful error if missing)
  # Otherwise, use tryGetInput to populate the docs when the input is available.
  conan-flake =
    if cfg.enable then config.lib.getInput inputArgs else config.lib.tryGetInput inputArgs;

  # Determine config root: prefer git.root, fallback to devenv.root
  configRoot = if config.git.root != null then config.git.root else config.devenv.root;

  conanSubmodule =
    if conan-flake != null then
    # We automatically configure Conan with the correct tree root for the project.
      conan-flake.lib.submoduleWith pkgs { inherit configRoot; }
    else
      lib.types.attrs;
in
{
  options.conan = {
    enable = lib.mkEnableOption "conan integration (through conan-flake)";

    config = lib.mkOption {
      description = "conan configuration.";
      type = conanSubmodule;
      default = { };
    };
  };

  config = lib.mkIf cfg.enable {

    # conan-flake exposes an `outputs.devShell` devShell by default that can be
    # used directly, or passed in the inputsFrom option as a means to compose
    # with other devShell modules.
    inputsFrom = [ cfg.config.outputs.devShell ];

  };
}
