{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cplusplus;

  inputArgs = {
    name = "conan-flake";
    url = "git+https://codeberg.org/tarcisio/conan-flake";
    attribute = "conan";
  };

  # When enabled, use getInput (throws helpful error if missing)
  # Otherwise, use tryGetInput to populate the docs when the input is available.
  conan-flake =
    if cfg.conan.enable then config.lib.getInput inputArgs else config.lib.tryGetInput inputArgs;

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
  options.languages.cplusplus = {
    enable = lib.mkEnableOption "tools for C++ development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.clang;
      defaultText = lib.literalExpression "pkgs.clang";
      description = "The C++ compiler to use.";
    };

    cmake = lib.mkOption {
      type = lib.types.submodule {
        options.package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.cmake;
          defaultText = lib.literalExpression "pkgs.cmake";
          description = "The CMake package to use.";
        };
      };
      description = "Configuration for cmake";
      default = { };
    };

    tools = {
      enable = lib.mkEnableOption "Standalone command line tools for C++ development" // {
        default = cfg.package.isClang;
        defaultText = lib.literalMD "Enabled by default for clang-based compilers";
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.clang-tools;
        defaultText = lib.literalExpression "pkgs.clang-tools";
        description = "The C++ command line tools package to use.";
      };
    };

    conan = {
      enable = lib.mkEnableOption "install conan";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.conan;
        defaultText = lib.literalExpression "pkgs.conan";
        description = "The conan package to use.";
      };
      config = lib.mkOption {
        type = conanSubmodule;
        description = "conan configuration.";
        default = { };
      };
      install.enable = lib.mkEnableOption "conan install during devenv initialisation";
    };

    lsp = {
      enable = lib.mkEnableOption "C++ Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.ccls;
        defaultText = lib.literalExpression "pkgs.ccls";
        description = "The C++ language server package to use.";
      };
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = with pkgs; [
        cfg.cmake.package
        cfg.package
      ]
      ++ lib.optional cfg.tools.enable cfg.tools.package
      ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional cfg.conan.enable cfg.conan.package;
    })

    #
    (lib.mkIf (cfg.enable && cfg.conan.enable) {
      languages.cplusplus.conan.config.stdenv = lib.mkDefault (if config.stdenv.hasCC then config.stdenv else pkgs.stdenv);
      languages.cplusplus.conan.config.package = lib.mkDefault cfg.conan.package;
      languages.cplusplus.conan.config.platformToolRequires = lib.mkDefault {
        cmake = cfg.cmake.package.version;
      };
      languages.cplusplus.conan.config.defaults.enable = lib.mkDefault false;
      languages.cplusplus.package = lib.mkDefault cfg.conan.config.stdenv.cc;
    })

    #
    (lib.mkIf (cfg.enable && cfg.conan.enable && cfg.conan.install.enable) {
      inputsFrom = [ cfg.conan.config.outputs.devShell ];
    })
  ];
}
