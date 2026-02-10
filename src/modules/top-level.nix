{ config, pkgs, lib, bootstrapPkgs ? null, devenvSandbox ? { enable = false; }, ... }:
let
  types = lib.types;

  # Returns a list of all the entries in a folder
  listEntries = path:
    map (name: path + "/${name}") (builtins.attrNames (builtins.readDir path));

  drvOrPackageToPaths = drvOrPackage:
    if drvOrPackage ? outputs then
      builtins.map (output: drvOrPackage.${output}) drvOrPackage.outputs
    else
      [ drvOrPackage ];

  profile = pkgs.buildEnv {
    name = "devenv-profile";
    paths = lib.flatten (builtins.map drvOrPackageToPaths config.packages);
    ignoreCollisions = true;
    ignoreSingleFileOutputs = true;
  };

  failedAssertions = builtins.map (x: x.message) (builtins.filter (x: !x.assertion) config.assertions);

  sandboxer = pkgs.rustPlatform.buildRustPackage {
    pname = "sandboxer";
    version = "0.0.1";
    src = pkgs.fetchFromGitHub {
      owner = "landlock-lsm";
      repo = "landlockconfig";
      rev = "8b6b59b339181f9fa1ec6f7889564ba154c1a47d";
      hash = "sha256-4LOauaC3eTLvERp9E7HIcunzkJ7HHcLkLAmaSbisr/c=";
    };
    # Upstream doesn't have a Cargo.lock file yet, so we provide one
    postUnpack = ''
      cp ${./landlockconfig.Cargo.lock} source/Cargo.lock
    '';
    cargoLock = {
      lockFile = ./landlockconfig.Cargo.lock;
    };
    installPhase = ''
      mkdir -p $out/bin
      cp target/*/release/examples/sandboxer $out/bin/
    '';
    cargoBuildFlags = [
      "--example"
      "sandboxer"
    ];
  };

  shellHook = pkgs.writeShellScriptBin "shellHook" config.enterShell;

  # Extract store paths from environment variable values
  # Only include values with proper Nix context (derivations or context strings)
  # Plain strings without context are ignored - exportReferencesGraph will reject them anyway
  extractEnvStorePaths = envAttrs:
    lib.filter
      (v: v != null)
      (lib.mapAttrsToList
        (name: value:
          if lib.isDerivation value then
            value
          else if lib.isString value && builtins.hasContext value then
            value
          else
            null
        )
        envAttrs
      );

  # Compute the closure of all packages that need to be accessible in the sandbox
  # This creates a derivation that uses exportReferencesGraph to get all dependencies
  sandboxer-settings =
    let
      # Extract store paths from config.env that have Nix context
      envStorePaths = extractEnvStorePaths config.env;

      # List of root packages whose closure we need
      closureRoots = lib.flatten [
        config.packages
        config.inputsFrom
        sandboxer
        config.stdenv
        shellHook
        envStorePaths
      ];

      # Create a derivation that computes the closure and generates the TOML config
      # We use exportReferencesGraph to get all transitive dependencies
      # Create a trivial derivation that references all closure roots (handles both files and directories)
      allRoots = pkgs.writeText "sandbox-closure-roots" (
        lib.concatStringsSep "\n" (map toString closureRoots)
      );
      mkSandboxConfig = pkgs.runCommand "sandboxer-settings.toml"
        {
          # exportReferencesGraph writes the closure of allRoots to a file
          exportReferencesGraph = [ "closure" allRoots ];
          nativeBuildInputs = [ pkgs.jq ];
        }
        ''
          # Start generating the TOML config
          cat > $out <<'HEADER'
          abi = 5

          [[path_beneath]]
          allowed_access = ["abi.read_write"]
          parent = [
            "${config.devenv.root}",
            "${config.devenv.runtime}",
            "${config.devenv.tmpdir}",
            "/proc",
            "/tmp",
            "/dev/tty",
            "/dev/null"
          ]

          [[path_beneath]]
          allowed_access = ["abi.read_execute"]
          parent = [
            "/proc/stat",
          HEADER

          # Extract, deduplicate, and format store paths
          grep '^/nix/store' closure | sort -u | sed 's|^|  "|; s|$|",|' >> $out

          echo "]" >> $out
        '';
    in
    mkSandboxConfig;
  sandbox = lib.optionalString devenvSandbox.enable "${sandboxer}/bin/sandboxer --toml ${sandboxer-settings} --";

  performAssertions =
    let
      formatAssertionMessage = message:
        let
          lines = lib.splitString "\n" message;
        in
        "- ${lib.concatStringsSep "\n  " lines}";
    in
    if failedAssertions != [ ]
    then
      throw ''
        Failed assertions:
        ${lib.concatStringsSep "\n" (builtins.map formatAssertionMessage failedAssertions)}
      ''
    else lib.trivial.showWarnings config.warnings;
in
{
  options = {
    env = lib.mkOption {
      type = types.submodule {
        freeformType = types.lazyAttrsOf types.anything;
      };
      description = "Environment variables to be exposed inside the developer environment.";
      default = { };
    };

    name = lib.mkOption {
      type = types.nullOr types.str;
      description = "Name of the project.";
      default = "devenv-shell";
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

    inputsFrom = lib.mkOption {
      type = types.listOf types.package;
      description = "A list of derivations whose build inputs will be merged into the shell environment.";
      default = [ ];
      example = lib.literalExpression ''
        [
          pkgs.hello
          (pkgs.python3.withPackages (ps: [ ps.numpy ps.pandas ]))
        ]
      '';
    };

    stdenv = lib.mkOption {
      type = types.package;
      description = "The stdenv to use for the developer environment.";
      default = pkgs.stdenv;
      defaultText = lib.literalExpression "pkgs.stdenv";

      # Remove the default apple-sdk on macOS.
      # Allow users to specify an optional SDK in `apple.sdk`.
      apply = stdenv:
        if stdenv.isDarwin
        then
          stdenv.override
            (prev: {
              extraBuildInputs =
                builtins.filter (x: !lib.hasPrefix "apple-sdk" x.pname) prev.extraBuildInputs;
            })
        else stdenv;

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
        example = lib.literalExpression "pkgs.apple-sdk_15";
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
      example = [{ assertion = false; message = "you can't enable this for that reason"; }];
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

      sandbox = lib.mkOption {
        type = types.submodule {
          options = {
            enable = lib.mkOption {
              type = types.bool;
              readOnly = true;
              description = ''
                Enable the sandbox. This option is controlled by the `sandbox.enable` setting
                in devenv.yaml and cannot be overridden in devenv.nix.
              '';
            };
          };
        };
        readOnly = true;
        default = devenvSandbox;
        description = "Sandbox configuration";
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
            # XDG_RUNTIME_DIR is the correct location for runtime files like sockets
            # per the XDG Base Directory Specification
            xdg = builtins.getEnv "XDG_RUNTIME_DIR";
            base = if xdg != "" then xdg else config.devenv.tmpdir;
          in
          "${base}/devenv-${shortHash}";
      };

      tmpdir = lib.mkOption {
        type = types.str;
        internal = true;
        # Used for TMPDIR override - should NOT use XDG_RUNTIME_DIR as that's
        # a small tmpfs meant for runtime files (sockets), not build artifacts
        default =
          let
            tmp = builtins.getEnv "TMPDIR";
          in
          if tmp != "" then tmp else "/tmp";
      };

      profile = lib.mkOption {
        type = types.package;
        internal = true;
      };
    };
  };

  imports = [
    ./profiles.nix
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
    ./machines.nix
    ./tests.nix
    ./cachix.nix
    ./tasks.nix
    ./changelogs.nix
    ./flake-compat.nix
  ]
  ++ (listEntries ./languages)
  ++ (listEntries ./services)
  ++ (listEntries ./integrations)
  ++ (listEntries ./process-managers)
  ;

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
        assertion = config.devenv.flakesIntegration || config.overlays == [ ] || (config.devenv.cli.version != null && lib.versionAtLeast config.devenv.cli.version "1.4.2");
        message = ''
          Using overlays requires devenv 1.4.2 or higher, while your current version is ${toString config.devenv.cli.version}.
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
    ]
    ++ lib.optional (config.apple.sdk != null) config.apple.sdk;

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
        # Use devenvSandbox directly from module args (bypasses config system, cannot be overridden)
        wrappedPackages = if devenvSandbox.enable then map wrapBinaries config.packages else config.packages;
        wrappedInputsFrom = if devenvSandbox.enable then map wrapBinaries config.inputsFrom else config.inputsFrom;
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
      in
      performAssertions (
        (pkgs.mkShell.override { stdenv = config.stdenv; }) ({
          inherit (config) hardeningDisable name;
          inputsFrom = wrappedInputsFrom;
          inherit buildInputs nativeBuildInputs;
          shellHook = ''
            ${lib.optionalString config.devenv.debug "set -x"}
            ${sandbox} "${shellHook}/bin/shellHook"
          '';
        } // config.env)
      );

    infoSections."env" = lib.mapAttrsToList (name: value: "${name}: ${toString value}") config.env;
    infoSections."packages" = builtins.map (package: package.name) (builtins.filter (package: !(builtins.elem package.name (builtins.attrNames config.scripts))) config.packages);

    ci = [ config.shell ];
    ciDerivation = pkgs.runCommand "ci" { } "echo ${toString config.ci} > $out";
  };
}
