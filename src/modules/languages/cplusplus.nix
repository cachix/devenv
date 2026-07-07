{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cplusplus;

  inputArgs = {
    name = "conan-flake";
    url = "git+https://codeberg.org/tarcisio/conan-flake";
    attribute = "conan";
  };

  relativePathType = lib.types.pathWith {
    inStore = false;
    absolute = false;
  };

  # When enabled, use getInput (throws helpful error if missing)
  # Otherwise, use tryGetInput to populate the docs when the input is available.
  conan-flake =
    if cfg.conan.enable then config.lib.getInput inputArgs else config.lib.tryGetInput inputArgs;

  # Determine config root: prefer devenv.root, fallback to git.root
  configRoot = if config.devenv.root != null then config.devenv.root else config.git.root;

  homeRoot = if relativePathType.check cfg.directory then configRoot else cfg.directory;

  homeDirectory = if relativePathType.check cfg.directory then cfg.directory else ".";

  conanSubmodule =
    if conan-flake != null then
      conan-flake.lib.submoduleWith lib
        {
          specialArgs = { inherit pkgs; };
        }
    else
      lib.types.attrs;
in
{
  options.languages.cplusplus = {
    enable = lib.mkEnableOption "tools for C++ development";

    directory = lib.mkOption {
      type = lib.types.str;
      default = configRoot;
      defaultText = lib.literalExpression "if config.devenv.root != null then config.devenv.root else config.git.root";
      description = ''
        The C++ project's root directory. Defaults to the root of the devenv
        project (or the root of the git tree, if no devenv root is set).
        Can be an absolute path or one relative to the root of the devenv
        project (or of the git tree, if no devenv root is set).
      '';
      example = "./directory";
    };

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
      packages = [
        cfg.cmake.package
        cfg.package
      ]
      ++ lib.optional cfg.tools.enable cfg.tools.package
      ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional cfg.conan.enable cfg.conan.package;
    })

    #
    (lib.mkIf (cfg.enable && cfg.conan.enable) {
      languages.cplusplus.conan.config.configRoot = lib.mkDefault null;

      languages.cplusplus.conan.config.homeRoot = lib.mkDefault homeRoot;

      languages.cplusplus.conan.config.homeDirectory = lib.mkDefault homeDirectory;

      languages.cplusplus.conan.config.package = lib.mkDefault cfg.conan.package;

      # conan-flake uses its stdenv option to figure out the compiler
      # infrastructure and feed Conan user settings and default profile from
      # what it can get from there. So try to use the one devenv is configured
      # with, _unless_ it lacks C/C++ compilaltion support - in which case fall
      # back to the system's default stdenv:
      languages.cplusplus.conan.config.stdenv = lib.mkDefault (
        if config.stdenv.hasCC
        then config.stdenv
        else pkgs.stdenv
      );

      # Tell Conan to use the already installed system-wide CMake when resolving
      # the dependencies on platform tools:
      languages.cplusplus.conan.config.profiles.platformToolRequires = lib.mkDefault {
        cmake = cfg.cmake.package.version;
      };

      # By default, conan-flake makes these tools available in the devShell, but
      # we're handling them here:
      languages.cplusplus.conan.config.devShell.tools = lib.mkDefault {
        conan = null; # cf. languages.cplusplus.conan.package
        cmake = null; # cf. languages.cplusplus.cmake.package

        # By default, the "${cfg.conan.config.stdenv.cc.cc.pname}" entry is set to
        # cfg.conan.config.stdenv.cc, that is, it would be equivalent to:
        # "${cfg.conan.config.stdenv.cc.cc.pname}" = cfg.conan.config.stdenv.cc;
        "${cfg.conan.config.stdenv.cc.cc.pname}" = null;
        # We will handle this with languages.cplusplus.package, cf. below:
      };

      languages.cplusplus.package = lib.mkDefault cfg.conan.config.stdenv.cc;

      #
      env = cfg.conan.config.devShell.env;
      packages = with cfg.conan.config.outputs.devShell; buildInputs ++ nativeBuildInputs;
    })

    #
    (lib.mkIf (cfg.enable && cfg.conan.enable && cfg.conan.install.enable) {
      languages.cplusplus.conan.config.wrappers = {
        conanLockFile = "conan.lock";
        conanInstall = true;
      };
      enterShell = ''
        ${cfg.conan.config.outputs.devShell.shellHook}
      '';
    })
  ];
}
