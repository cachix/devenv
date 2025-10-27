{ inputs
, pkgs
, lib
, config
, ...
}:
{
  env = {
    # The path to the eval cache database (for migrations)
    DATABASE_URL = "sqlite:.devenv/nix-eval-cache.db";

    # The Nix CLI for devenv to use when run with `cargo run`.
    DEVENV_NIX = inputs.nix.packages.${pkgs.stdenv.system}.nix-cli;

    RUST_LOG = "devenv=debug";
    RUST_LOG_SPAN_EVENTS = "full";
  };

  # Configure Claude Code
  claude.code = {
    enable = true;
    permissions = {
      WebFetch = {
        allow = [
          "domain:github.com"
          "domain:docs.rs"
          "domain:docs.anthropic.com"
        ];
      };
      Bash = {
        allow = [
          "rg:*"
          "cargo test:*"
          "nix search:*"
          "devenv-run-tests:*"
          "nix-instantiate:*"
        ];
      };
    };
  };

  # Project dependencies
  packages = [
    pkgs.cairo
    pkgs.git
    pkgs.xorg.libxcb
    pkgs.yaml2json
    pkgs.tesh
    pkgs.watchexec
    pkgs.openssl
    pkgs.sqlx-cli
    pkgs.process-compose
    pkgs.cargo-outdated # Find outdated crates
    pkgs.cargo-machete # Find unused crates
    pkgs.cargo-edit # Adds the set-version command
    pkgs.protobuf # snix
    pkgs.dbus # secretspec
    # Force compilation from source instead of binary cache
    (pkgs.hello.overrideAttrs (old: {
      preferLocalBuild = true;
      allowSubstitutes = false;
    }))
  ];

  languages = {
    # For developing the Nix modules
    nix.enable = true;

    # For developing the devenv CLI
    rust.enable = true;
  };

  devcontainer = {
    enable = true;
    settings.customizations.vscode.extensions = [ "jnoortheen.nix-ide" ];
  };

  difftastic.enable = true;

  scripts.devenv-test-cli = {
    description = "Test devenv CLI.";
    exec = ''
      set -xe
      set -o pipefail

      tmp="$(mktemp -d)"
      devenv init "$tmp"
      pushd "$tmp"
        devenv version
        devenv --override-input devenv path:${config.devenv.root}?dir=src/modules test
      popd
      rm -rf "$tmp"

      # Test devenv init with target path
      tmp="$(mktemp -d)"
      pushd "$tmp"
        devenv init target
        test -z "$(ls -A1 | grep -v target)"
        pushd target
          devenv --override-input devenv path:${config.devenv.root}?dir=src/modules test
        popd
      popd
      rm -rf "$tmp"

      # Test devenv integrated into bare Nix flake
      tmp="$(mktemp -d)"
      pushd "$tmp"
        nix flake init --template ''${DEVENV_ROOT}#simple
        nix flake update \
          --override-input devenv ''${DEVENV_ROOT}
        nix develop --accept-flake-config --no-pure-eval --command echo nix-develop started succesfully |& tee ./console
        grep -F 'nix-develop started succesfully' <./console
        grep -F "$(${lib.getExe pkgs.hello})" <./console

        # Assert that nix-develop fails in pure mode.
        if nix develop --command echo nix-develop started in pure mode |& tee ./console
        then
          echo "nix-develop was able to start in pure mode. This is explicitly not supported at the moment."
          exit 1
        fi
        grep -F 'devenv was not able to determine the current directory.' <./console
      popd
      rm -rf "$tmp"

      # Test devenv integrated into flake-parts Nix flake
      tmp="$(mktemp -d)"
      pushd "$tmp"
        nix flake init --template ''${DEVENV_ROOT}#flake-parts
        nix flake update \
          --override-input devenv ''${DEVENV_ROOT}
        nix develop --accept-flake-config --override-input devenv-root "file+file://"<(printf %s "$PWD") --command echo nix-develop started succesfully |& tee ./console
        grep -F 'nix-develop started succesfully' <./console
        grep -F "$(${lib.getExe pkgs.hello})" <./console
        # Test that a container can be built
        if [ "$(uname)" = "Linux" ]
        then
          nix build --override-input devenv-root "file+file://"<(printf %s "$PWD") --accept-flake-config --show-trace .#container-processes
        fi
      popd
      rm -rf "$tmp"
    '';
  };

  git-hooks.hooks = {
    nixpkgs-fmt.enable = true;
    rustfmt.enable = true;
    markdownlint = {
      settings.configuration = {
        MD013 = {
          line_length = 120;
        };
        MD033 = false;
        MD034 = false;
      };
    };
  };
}
