{
  config,
  pkgs,
  lib,
  bootstrapPkgs ? null,
  ...
}:
let
  types = lib.types;
  # Returns a list of all the entries in a folder
  listEntries = path: map (name: path + "/${name}") (builtins.attrNames (builtins.readDir path));

  drvOrPackageToPaths =
    drvOrPackage:
    if drvOrPackage ? outputs then
      builtins.map (output: drvOrPackage.${output}) drvOrPackage.outputs
    else
      [ drvOrPackage ];
  profile = pkgs.buildEnv {
    name = "devenv-profile";
    paths = lib.flatten (builtins.map drvOrPackageToPaths config.packages);
    ignoreCollisions = true;
  };

  failedAssertions = builtins.map (x: x.message) (
    builtins.filter (x: !x.assertion) config.assertions
  );

  performAssertions =
    let
      formatAssertionMessage =
        message:
        let
          lines = lib.splitString "\n" message;
        in
        "- ${lib.concatStringsSep "\n  " lines}";
    in
    if failedAssertions != [ ] then
      throw ''
        Failed assertions:
        ${lib.concatStringsSep "\n" (builtins.map formatAssertionMessage failedAssertions)}
      ''
    else
      lib.trivial.showWarnings config.warnings;

  sandboxer = pkgs.rustPlatform.buildRustPackage {
    pname = "sandboxer";
    version = "0.0.1";
    src = pkgs.fetchFromGitHub {
      #owner = "landlock-lsm";
      owner = "lorenzbischof"; # repository does not contain a lockfile yet
      repo = "landlockconfig";
      rev = "main";
      hash = "sha256-odG+YsK3+YJdWS6ATJ2YcAWn5rwTNInH0EleI3/5jG8=";
    };
    installPhase = ''
      mkdir -p $out/bin
      cp target/*/release/examples/sandboxer $out/bin/
    '';
    cargoBuildFlags = [
      "--example"
      "sandboxer"
    ];
    cargoHash = "sha256-YVty2x8jz/TNfCMks9LFN70mkSrYIFng3enLYt2REBo=";
  };
  sandboxer-settings = pkgs.writers.writeTOML "sandboxer.toml" {
    ruleset = [
      { handled_access_fs = [ "v5.all" ]; }
    ];
    path_beneath = [
      {
        allowed_access = [ "v5.read_write" ];
        parent_fd = [
          config.devenv.root
          config.devenv.runtime
          config.devenv.tmpdir
          "/proc"
          "/tmp"
          "/dev/tty"
          "/dev/null"
        ];
      }
      {
        allowed_access = [ "v5.read_execute" ];
        parent_fd = [
          "/nix"
          "/proc/stat"
        ];
      }
    ];
  };
  sandbox = lib.optionalString config.devenv.experimental_sandbox "${sandboxer}/bin/sandboxer --toml ${sandboxer-settings}";
in
{
  options = {
    env = lib.mkOption {
      type = types.submoduleWith {
        modules = [
          (env: {
            config._module.freeformType = types.lazyAttrsOf types.anything;
          })
        ];
      };
      description = "Environment variables to be exposed inside the developer environment.";
      default = { };
    };

    name = lib.mkOption {
      type = types.nullOr types.str;
      description = "Name of the project.";
      default = null;
    };

    enterShell = lib.mkOption {
      type = types.lines;
      description = "Bash code to execute when entering the shell.";
      default = "";
    };

    overlays = lib.mkOption {
      type = types.listOf (types.functionTo (types.functionTo types.attrs));
      description = "List of overlays to apply to pkgs. Each overlay is a function that takes two arguments: final and prev. Supported by devenv 1.4.2 or newer.";
      default = [ ];
      example = lib.literalExpression ''
        [
          (final: prev: {
            hello = prev.hello.overrideAttrs (oldAttrs: {
              patches = (oldAttrs.patches or []) ++ [ ./hello-fix.patch ];
            });
          })
        ]
      '';
    };

    packages = lib.mkOption {
      type = types.listOf types.package;
      description = "A list of packages to expose inside the developer environment. Search available packages using ``devenv search NAME``.";
      default = [ ];
    };

    stdenv = lib.mkOption {
      type = types.package;
      description = "The stdenv to use for the developer environment.";
      default = pkgs.stdenv;
      defaultText = lib.literalExpression "pkgs.stdenv";

      # Remove the default apple-sdk on macOS.
      # Allow users to specify an optional SDK in `apple.sdk`.
      apply =
        stdenv:
        if stdenv.isDarwin then
          stdenv.override (prev: {
            extraBuildInputs = builtins.filter (x: !lib.hasPrefix "apple-sdk" x.pname) prev.extraBuildInputs;
          })
        else
          stdenv;

    };

    apple = {
      sdk = lib.mkOption {
        type = types.nullOr types.package;
        description = ''
          The Apple SDK to add to the developer environment on macOS.

          If set to `null`, the system SDK can be used if the shell allows access to external environment variables.
        '';
        default = if pkgs.stdenv.isDarwin then pkgs.apple-sdk else null;
        defaultText = lib.literalExpression "if pkgs.stdenv.isDarwin then pkgs.apple-sdk else null";
      };
    };

    unsetEnvVars = lib.mkOption {
      type = types.listOf types.str;
      description = "A list of removed environment variables to make the shell/direnv more lean.";
      # manually determined with knowledge from https://nixos.wiki/wiki/C
      default = [
        "HOST_PATH"
        "NIX_BUILD_CORES"
        "__structuredAttrs"
        "buildInputs"
        "buildPhase"
        "builder"
        "depsBuildBuild"
        "depsBuildBuildPropagated"
        "depsBuildTarget"
        "depsBuildTargetPropagated"
        "depsHostHost"
        "depsHostHostPropagated"
        "depsTargetTarget"
        "depsTargetTargetPropagated"
        "dontAddDisableDepTrack"
        "doCheck"
        "doInstallCheck"
        "nativeBuildInputs"
        "out"
        "outputs"
        "patches"
        "phases"
        "preferLocalBuild"
        "propagatedBuildInputs"
        "propagatedNativeBuildInputs"
        "shell"
        "shellHook"
        "stdenv"
        "strictDeps"
      ];
    };

    shell = lib.mkOption {
      type = types.package;
      internal = true;
    };

    ci = lib.mkOption {
      type = types.listOf types.package;
      internal = true;
    };

    ciDerivation = lib.mkOption {
      type = types.package;
      internal = true;
    };

    assertions = lib.mkOption {
      type = types.listOf types.unspecified;
      internal = true;
      default = [ ];
      example = [
        {
          assertion = false;
          message = "you can't enable this for that reason";
        }
      ];
      description = ''
        This option allows modules to express conditions that must
        hold for the evaluation of the configuration to succeed,
        along with associated error messages for the user.
      '';
    };

    hardeningDisable = lib.mkOption {
      type = types.listOf types.str;
      internal = true;
      default = [ ];
      example = [ "fortify" ];
      description = ''
        This options allows modules to disable selected hardening modules.
        Currently used only for Go
      '';
    };

    warnings = lib.mkOption {
      type = types.listOf types.str;
      internal = true;
      default = [ ];
      example = [ "you should fix this or that" ];
      description = ''
        This option allows modules to express warnings about the
        configuration. For example, `lib.mkRenamedOptionModule` uses this to
        display a warning message when a renamed option is used.
      '';
    };

    devenv = {
      root = lib.mkOption {
        type = types.str;
        internal = true;
        default = builtins.getEnv "PWD";
      };

      dotfile = lib.mkOption {
        type = types.str;
        internal = true;
      };

      state = lib.mkOption {
        type = types.str;
        internal = true;
      };

      experimental_sandbox = lib.mkOption {
        type = types.bool;
        default = false;
        description = "Enable the experimental sandbox";
      };

      runtime = lib.mkOption {
        type = types.str;
        internal = true;
        # The path has to be
        # - unique to each DEVENV_STATE to let multiple devenv environments coexist
        # - deterministic so that it won't change constantly
        # - short so that unix domain sockets won't hit the path length limit
        # - free to create as an unprivileged user across OSes
        default =
          let
            hashedRoot = builtins.hashString "sha256" config.devenv.state;
            # same length as git's abbreviated commit hashes
            shortHash = builtins.substring 0 7 hashedRoot;
          in
          "${config.devenv.tmpdir}/devenv-${shortHash}";
      };

      tmpdir = lib.mkOption {
        type = types.str;
        internal = true;
        default =
          let
            xdg = builtins.getEnv "XDG_RUNTIME_DIR";
            tmp = builtins.getEnv "TMPDIR";
          in
          if xdg != "" then
            xdg
          else if tmp != "" then
            tmp
          else
            "/tmp";
      };

      profile = lib.mkOption {
        type = types.package;
        internal = true;
      };
    };
  };

  imports =
    [
      ./info.nix
      ./outputs.nix
      ./files.nix
      ./processes.nix
      ./outputs.nix
      ./scripts.nix
      ./update-check.nix
      ./containers.nix
      ./debug.nix
      ./lib.nix
      ./tests.nix
      ./cachix.nix
      ./tasks.nix
    ]
    ++ (listEntries ./languages)
    ++ (listEntries ./services)
    ++ (listEntries ./integrations)
    ++ (listEntries ./process-managers);

  config = {
    assertions = [
      {
        assertion = config.devenv.root != "";
        message = ''
          devenv was not able to determine the current directory.

          See https://devenv.sh/guides/using-with-flakes/ how to use it with flakes.
        '';
      }
      {
        assertion =
          config.devenv.flakesIntegration
          || config.overlays == [ ]
          || lib.versionAtLeast config.devenv.cliVersion "1.4.2";
        message = ''
          Using overlays requires devenv 1.4.2 or higher, while your current version is ${config.devenv.cliVersion}.
        '';
      }
    ];
    # use builtins.toPath to normalize path if root is "/" (container)
    devenv.state = builtins.toPath (config.devenv.dotfile + "/state");
    devenv.dotfile = lib.mkDefault (builtins.toPath (config.devenv.root + "/.devenv"));
    devenv.profile = profile;

    env.DEVENV_PROFILE = config.devenv.profile;
    env.DEVENV_STATE = config.devenv.state;
    env.DEVENV_RUNTIME = config.devenv.runtime;
    env.DEVENV_DOTFILE = config.devenv.dotfile;
    env.DEVENV_ROOT = config.devenv.root;

    packages = [
      # needed to make sure we can load libs
      pkgs.pkg-config
    ] ++ lib.optional (config.apple.sdk != null) config.apple.sdk;

    enterShell = lib.mkBefore ''
      export PS1="\[\e[0;34m\](devenv)\[\e[0m\] ''${PS1-}"

      # override temp directories after "nix develop"
      for var in TMP TMPDIR TEMP TEMPDIR; do
        if [ -n "''${!var-}" ]; then
          export "$var"=${config.devenv.tmpdir}
        fi
      done
      if [ -n "''${NIX_BUILD_TOP-}" ]; then
        unset NIX_BUILD_TOP
      fi

      # set path to locales on non-NixOS Linux hosts
      ${lib.optionalString (pkgs.stdenv.isLinux && (pkgs.glibcLocalesUtf8 != null)) ''
        if [ -z "''${LOCALE_ARCHIVE-}" ]; then
          export LOCALE_ARCHIVE=${pkgs.glibcLocalesUtf8}/lib/locale/locale-archive
        fi
      ''}

      # direnv helper
      if [ ! type -p direnv &>/dev/null && -f .envrc ]; then
        echo "An .envrc file was detected, but the direnv command is not installed."
        echo "To use this configuration, please install direnv: https://direnv.net/docs/installation.html"
      fi

      mkdir -p "$DEVENV_STATE"
      if [ ! -L "$DEVENV_DOTFILE/profile" ] || [ "$(${pkgs.coreutils}/bin/readlink $DEVENV_DOTFILE/profile)" != "${profile}" ]
      then
        ln -snf ${profile} "$DEVENV_DOTFILE/profile"
      fi
      unset ${lib.concatStringsSep " " config.unsetEnvVars}

      mkdir -p ${lib.escapeShellArg config.devenv.runtime}
      ln -snf ${lib.escapeShellArg config.devenv.runtime} ${lib.escapeShellArg config.devenv.dotfile}/run
    '';

    shell =
      let
        # `mkShell` merges `packages` into `nativeBuildInputs`.
        # This distinction is generally not important for devShells, except when it comes to setup hooks and their run order.
        # On macOS, the default apple-sdk is added to stdenv via `extraBuildInputs`.
        # If we don't remove it from stdenv, then its setup hooks will clobber any SDK added to `packages`.
        isAppleSDK = pkg: builtins.match ".*apple-sdk.*" (pkg.pname or "") != null;
        partitionedPkgs = builtins.partition isAppleSDK wrappedPackages;
        buildInputs = partitionedPkgs.right;
        nativeBuildInputs = partitionedPkgs.wrong;
        wrappedPackages = map wrapBinaries config.packages;
        wrapBinaries =
          pkg:
          pkgs.stdenv.mkDerivation {
            name = "wrapped-${pkg.name}";
            src = [ pkg ];
            buildInputs = [ pkgs.makeWrapper ];

            postBuild = ''
              mkdir -p $out/bin
              for bin in $src/bin/*; do
                if [ -x "$bin" ] && [ -f "$bin" ]; then
                  echo "exec ${sandbox} $bin \"\$@\"" > $out/bin/$(basename $bin)
                  chmod +x $out/bin/$(basename $bin)
                fi
              done
            '';
          };
        shellHook = pkgs.writeShellScriptBin "shellHook" config.enterShell;
      in
      performAssertions (
        (pkgs.mkShell.override { stdenv = config.stdenv; }) (
          {
            name = "devenv-shell";
            hardeningDisable = config.hardeningDisable;
            inherit buildInputs nativeBuildInputs;
            shellHook = ''
              ${lib.optionalString config.devenv.debug "set -x"}
              ${sandbox} ${shellHook}/bin/shellHook
            '';
          }
          // config.env
        )
      );

    infoSections."env" = lib.mapAttrsToList (name: value: "${name}: ${toString value}") config.env;
    infoSections."packages" = builtins.map (package: package.name) (
      builtins.filter (
        package: !(builtins.elem package.name (builtins.attrNames config.scripts))
      ) config.packages
    );

    _module.args.pkgs = bootstrapPkgs.appendOverlays config.overlays;
    _module.args.sandbox = sandbox;

    ci = [ config.shell ];
    ciDerivation = pkgs.runCommand "ci" { } "echo ${toString config.ci} > $out";
  };
}
