{
  scenario ? "normal",
  nixpkgs ? <nixpkgs>,
  envLibrary ? ../../src/modules/lib/env.nix,
}:
let
  pkgs = import nixpkgs { };
  inherit (pkgs) lib;
  env = import envLibrary { inherit lib; };
  evaluated = lib.evalModules {
    modules = [
      {
        options.env = lib.mkOption {
          type = lib.types.submodule { freeformType = lib.types.lazyAttrsOf env.type; };
          default = { };
        };
      }
      {
        env = {
          CC = env.set "zig cc";
          CONFIG_SHELL = env.set "/custom/bin/sh";
          PKG_CONFIG_PATH = env.prepend ":" [ "/custom/lib/pkgconfig" ];
          NIX_CFLAGS_COMPILE = env.append " " [ "-DCUSTOM" ];
          NIX_LDFLAGS = env.append " " [ "-L/custom/lib" ];
          SEQUENCE = env.combine [
            (env.beforeSetup "initial")
            (env.prepend ":" [ "first" ])
            (env.append ":" [ "last" ])
          ];
          BOTH = env.combine [
            (env.beforeSetup "before")
            (env.set "after")
          ];
          EMPTY = env.set "";
          EXISTING_EMPTY = env.combine [
            (env.beforeSetup "")
            (env.default "fallback")
          ];
          UNSET_DEFAULT = env.default "fallback";
          ZERO_SEPARATOR = env.combine [
            (env.beforeSetup "middle")
            (env.prepend "" [ "left" ])
            (env.append "" [ "right" ])
          ];
          REPLACE_AND_EXTEND = env.combine [
            (env.prepend ":" [ "prefix" ])
            (env.set "base")
            (env.append ":" [ "suffix" ])
          ];
          DEFAULT_AND_EXTEND = env.combine [
            (env.prepend ":" [ "prefix" ])
            (env.default "fallback")
            (env.append ":" [ "suffix" ])
          ];
          PREPEND_UNSET = env.prepend ":" [ "prefix" ];
          APPEND_UNSET = env.append ":" [ "suffix" ];
          QUOTED = env.set "literal ' \" $HOME $(false) `false`\nsecond line\n";
          ODD_SEPARATOR = env.combine [
            (env.beforeSetup "middle")
            (env.prepend "$(false)\"" [ "left" ])
          ];
          TYPED_LIST = env.set [
            "one"
            [
              "two"
              "three"
            ]
          ];
          TYPED_BOOL = env.set true;
          TYPED_FALSE = env.set false;
          TYPED_PATH = env.set ./probe.nix;
          TYPED_PACKAGE = env.set pkgs.zlib;
          LEGACY_LIST = [
            "one"
            "two"
          ];
          LEGACY_BOOL = true;
          LEGACY_NULL = null;
          PROBE_COMPILER = lib.getExe pkgs.stdenv.cc;
        }
        // lib.optionalAttrs (scenario == "large") {
          LARGE_ONE = env.set (lib.concatStrings (lib.replicate 70000 "a"));
          LARGE_TWO = env.set (lib.concatStrings (lib.replicate 70000 "b"));
        };
      }
      { env.SEQUENCE = env.prepend ":" [ "second" ]; }
      { env.SEQUENCE = lib.mkBefore (env.prepend ":" [ "earliest" ]); }
      { env.SEQUENCE = lib.mkAfter (env.append ":" [ "latest" ]); }
    ];
  };
  resolved = env.resolve evaluated.config.env;
  base = pkgs.mkShell (
    {
      name = "devenv-env-helpers-${scenario}";
      packages = [
        pkgs.pkg-config
        pkgs.zlib
      ];
      preHook = ''
        # Verify that initial values really are available to setup hooks.
        export SETUP_SAW_BOTH="$BOTH"
        export BOTH="changed-by-setup"
        export SEQUENCE="hook:$SEQUENCE"
        export CC="prehook-cc"
        lateHook() { export CC="late-package-cc"; }
        postHooks+=( lateHook )
      '';
    }
    // resolved.initial
  );
  # Prototype integration point: wrap setup so final operations run after
  # postHook, postHooks, and userHook. The wrapper is independent of env values.
  # A production implementation must also preserve stdenv's broader interface.
  wrapper = pkgs.writeTextDir "setup" ''
    source ${pkgs.stdenv}/setup
    source "$devenvEnvFinalPath"
    unset devenvEnvFinalPath
  '';
in
lib.overrideDerivation base (_: {
  stdenv = wrapper;
  devenvEnvFinal = resolved.finalScript;
  passAsFile = (base.drvAttrs.passAsFile or [ ]) ++ [ "devenvEnvFinal" ];
  userHook = "export CC=late-user-cc";
})
