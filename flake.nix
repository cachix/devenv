{
  description = "devenv.sh - Fast, Declarative, Reproducible, and Composable Developer Environments";

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw= cachix.cachix.org-1:eWNHQldwUO7G2VkjpnjDbWwy4KQ/HNxht7H4SSoMckM=";
    extra-substituters = "https://devenv.cachix.org https://cachix.cachix.org";
  };

  # this needs to be rolling so we're testing what most devs are using
  inputs.nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
  inputs.git-hooks = {
    url = "github:cachix/git-hooks.nix";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
    };
  };
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };
  inputs.flake-parts = {
    url = "github:hercules-ci/flake-parts";
    inputs = {
      nixpkgs-lib.follows = "nixpkgs";
    };
  };
  inputs.nix = {
    url = "github:cachix/nix/devenv-2.34";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
      flake-parts.follows = "flake-parts";
      git-hooks-nix.follows = "git-hooks";
      nixpkgs-23-11.follows = "";
      nixpkgs-regression.follows = "";
    };
  };
  inputs.cachix = {
    url = "github:cachix/cachix/latest";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
      git-hooks.follows = "git-hooks";
      devenv.follows = "";
    };
  };
  inputs.nixd = {
    url = "github:nix-community/nixd";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-parts.follows = "flake-parts";
    };
  };
  inputs.crate2nix = {
    # https://github.com/nix-community/crate2nix/issues/439
    url = "github:rossng/crate2nix/ba5dd398e31ee422fbe021767eb83b0650303a6e";
    flake = false;
  };
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  inputs.ghostty = {
    url = "github:ghostty-org/ghostty";
    flake = false;
  };

  outputs =
    { self
    , nixpkgs
    , git-hooks
    , nix
    , ...
    }@inputs:
    let
      systems = [
        "x86_64-linux"
        "i686-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          overlays = [
            inputs.rust-overlay.overlays.default
            # Exposes `nixComponents2` (the component scope) so we can rebuild the
            # Nix C/C++ libraries below. `overlays.default` only sets `nix`.
            inputs.nix.overlays.internal
            (final: prev: {
              inherit (inputs.cachix.packages.${system}) cachix;
              # [static-link-spike] Build the Nix C++/C-API libraries with
              # default_library=static so they link *into* the devenv binary
              # instead of as ~14 shared objects. This removes the Nix cluster
              # from the dynamic closure and the bulk of startup-time symbol
              # resolution (do_lookup_x). External deps (boost, curl, …) stay
              # dynamic — default_library only affects Nix's own meson libs.
              nix =
                # Under pkgsStatic (musl) the fork already builds the Nix libs
                # static and handles LTO; just expose nix-cli + the C-API libs.
                # Disable S3/AWS though: it's on by default, and its
                # aws-c-* / aws-crt-cpp archives (CMake, no .pc) aren't on the
                # final static link line, so libnixstore's aws_* references go
                # unresolved. S3 doesn't affect startup; re-enable and link the
                # aws-c-* stack once the startup win is confirmed.
                # On glibc, rebuild them static ourselves (the Tier 1 override).
                if prev.stdenv.hostPlatform.isStatic then
                  let
                    staticComponents = prev.nixComponents2.overrideScope (
                      _finalScope: prevScope: {
                        nix-store = prevScope.nix-store.override { withAWS = false; };
                      }
                    );
                  in
                  staticComponents.nix-cli
                  // { libs = staticComponents.nix-everything.libs; }
                else
                  let
                    staticComponents =
                      (prev.nixComponents2.overrideScope (
                        _finalScope: prevScope: {
                          # TEMP for the startup measurement only: S3 doesn't affect
                          # startup (static archives resolve at link time), and
                          # aws-crt-cpp is a CMake dep with no .pc, so keeping S3
                          # needs the aws-c-* stack linked explicitly. Re-enable S3
                          # and link the aws libs once the startup win is confirmed.
                          nix-store = prevScope.nix-store.override { withAWS = false; };
                        }
                      )).overrideAllMesonComponents (
                        _finalAttrs: prevAttrs: {
                          mesonFlags = (prevAttrs.mesonFlags or [ ]) ++ [
                            (prev.lib.mesonOption "default_library" "static")
                          ];
                          # A static lib's generated .pc lists its buildInputs under
                          # Requires.private; propagate them so downstream components'
                          # pkg-config lookups (and the final devenv link) resolve the
                          # transitive deps (libblake3, boost, …).
                          propagatedBuildInputs =
                            (prevAttrs.propagatedBuildInputs or [ ]) ++ (prevAttrs.buildInputs or [ ]);
                          # The fork enables LTO for release builds (packaging/components.nix)
                          # but already disables it for `isStatic`, knowing LTO+static breaks.
                          # default_library=static on a glibc stdenv doesn't set isStatic, so
                          # it slips past and GCC 15 ICEs building nix-expr. Append after the
                          # fork's snippet so b_lto=false wins. LTO doesn't affect startup.
                          preConfigure = (prevAttrs.preConfigure or "") + ''
                            appendToVar mesonFlags "-Db_lto=false"
                          '';
                        }
                      );
                  in
                  staticComponents.nix-cli // { libs = staticComponents.nix-everything.libs; };
              nixd = inputs.nixd.packages.${system}.nixd;
              crate2nix = final.callPackage "${inputs.crate2nix}/crate2nix/default.nix" { };
              libghostty-vt = final.callPackage "${inputs.ghostty}/nix/libghostty-vt.nix" { };
            })
          ];
          pkgs = import nixpkgs { inherit overlays system; };
          gitRev = self.shortRev or (self.dirtyShortRev or "");
          # Use stable Rust from rust-overlay for crate2nix builds
          # (nixpkgs' buildRustCrate uses Rust 1.73 which is too old for some deps)
          rustToolchain = pkgs.rust-bin.stable.latest.default;
          workspace = pkgs.callPackage ./nix/workspace.nix {
            inherit gitRev;
            rustc = rustToolchain;
            cargo = rustToolchain;
          };

          # [tier2] Fully static (musl) build: every dep links into one binary,
          # no dynamic Nix/external libs, to reach the startup floor and let the
          # shell hook drop its caching.
          pkgsStatic = pkgs.pkgsStatic;
          rustToolchainStatic = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "x86_64-unknown-linux-musl" ];
          };
          # [tier2] libghostty-vt links libc++ for its vendored simdutf, which
          # makes Zig build its bundled libc++ for the target. Ghostty's nix
          # build sets `dontSetZigDefaultFlags` and passes no `-Dtarget`, so
          # Zig builds for `native-native` — and building libc++ for the
          # *native-detected* musl fails (libc++ <__locale> references ctype
          # masks the detected musl doesn't expose). Passing an explicit
          # `-Dtarget=<arch>-linux-musl` makes Zig use its own known-good musl
          # config, so libc++ (and thus SIMD) builds cleanly. The simd C++ is
          # compiled SIMDUTF_NO_LIBCXX/-fno-exceptions/-fno-rtti, so the static
          # archive we link references no libc++ symbols at runtime.
          libghosttyVtStatic = pkgsStatic.libghostty-vt.overrideAttrs (old: {
            zigBuildFlags = old.zigBuildFlags ++ [
              "-Dtarget=${pkgsStatic.stdenv.hostPlatform.parsed.cpu.name}-linux-musl"
            ];
          });
          workspaceStatic = pkgsStatic.callPackage ./nix/workspace.nix {
            inherit gitRev;
            rustc = rustToolchainStatic;
            cargo = rustToolchainStatic;
            buildStatic = true;
            libghostty-vt = libghosttyVtStatic;
          };
        in
        {
          inherit (workspace.crates) devenv devenv-tasks;
          devenv-static = workspaceStatic.crates.devenv;
          default = self.packages.${system}.devenv;
          crate2nix = pkgs.crate2nix;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          devenv-image = import ./containers/devenv/image.nix {
            inherit pkgs;
            inherit (self.packages.${system}) devenv;
          };
        }
      );

      modules = ./src/modules;

      templates =
        let

          flake-parts = {
            path = ./templates/flake-parts;
            description = "A flake with flake-parts, direnv and devenv.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };

          flake = {
            path = ./templates/flake;
            description = "A direnv supported Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };

          terraform = {
            path = ./templates/terraform;
            description = "A Terraform Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };
        in
        {
          inherit flake flake-parts terraform;
          simple = flake; # Backwards compatibility
          default = self.templates.flake;
        };

      flakeModule = self.flakeModules.default; # Backwards compatibility
      flakeModules = {
        default = import ./flake-module.nix self;
        readDevenvRoot =
          { inputs, lib, ... }:
          {
            config =
              let
                devenvRootFileContent =
                  if inputs ? devenv-root then builtins.readFile inputs.devenv-root.outPath else "";
              in
              lib.mkIf (devenvRootFileContent != "") {
                devenv.root = devenvRootFileContent;
              };
          };
      };

      lib = {
        mkConfig = args: (self.lib.mkEval args).config;

        mkEval =
          args@{ pkgs
          , inputs
          , modules
          , lib ? pkgs.lib
          ,
          }:
          let
            # TODO: deprecate default git-hooks input
            defaultInputs = { inherit git-hooks; };
            finalInputs = defaultInputs // inputs;

            specialArgs = finalInputs // {
              inputs = finalInputs;
            };

            modules = [
              (self.modules + /top-level.nix)
              (
                { config, ... }:
                {
                  # Configure overlays
                  _module.args.pkgs = pkgs.appendOverlays config.overlays;
                  # Enable the flakes integration
                  devenv.flakesIntegration = true;
                  # Disable CLI version checks
                  devenv.warnOnNewVersion = false;
                }
              )
            ]
            ++ args.modules;

            project = lib.evalModules {
              class = "devenv";
              inherit modules specialArgs;
            };
          in
          project;

        mkShell =
          args:
          let
            config = self.lib.mkConfig args;
          in
          config.shell
          // {
            inherit config;
            ci = config.ciDerivation;
          };
      };

      overlays.default = final: prev: {
        devenv = self.packages.${prev.system}.default;
      };
    };
}
