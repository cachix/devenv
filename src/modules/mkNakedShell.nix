# copied from https://github.com/amarshall/devshell/blob/917a891e98ca26c006a6be1fdb3335a74720c237/nix/mkNakedShell.nix#L1
# to support inputDerivation for easy CI caching as well: https://github.com/numtide/devshell/pull/226
{ bashInteractive
, coreutils
, system
, writeTextFile
, lib
}:
let
  bashPath = "${bashInteractive}/bin/bash";
  stdenv = writeTextFile {
    name = "naked-stdenv";
    destination = "/setup";
    text = ''
      # Fix for `nix develop`
      : ''${outputs:=out}

      runHook() {
        eval "$shellHook"
        unset runHook
      }
    '';
  };
in
{ name
, # A path to a buildEnv that will be loaded by the shell.
  # We assume that the buildEnv contains an ./env.bash script.
  profile
, env ? { }
, shellHook ? ""
, meta ? { }
, passthru ? { }
}:
let
  # simpler version of https://github.com/numtide/devshell/blob/20d50fc6adf77fd8a652fc824c6e282d7737b85d/modules/env.nix#L41
  envToBash = name: value: "export ${name}=${lib.escapeShellArg (toString value)}";
  startupEnv = lib.concatStringsSep "\n" (lib.mapAttrsToList envToBash env);
  derivationArg = {
    inherit name system;

    # `nix develop` actually checks and uses builder. And it must be bash.
    builder = bashPath;

    # Bring in the dependencies on `nix-build`
    args = [ "-ec" "${coreutils}/bin/ln -s ${profile} $out; exit 0" ];

    # $stdenv/setup is loaded by nix-shell during startup.
    # https://github.com/nixos/nix/blob/377345e26f1ac4bbc87bb21debcc52a1d03230aa/src/nix-build/nix-build.cc#L429-L432
    stdenv = stdenv;

    # The shellHook is loaded directly by `nix develop`. But nix-shell
    # requires that other trampoline.
    shellHook = ''
      # Remove all the unnecessary noise that is set by the build env
      unset NIX_BUILD_TOP NIX_BUILD_CORES NIX_STORE
      unset TEMP TEMPDIR TMP TMPDIR
      # $name variable is preserved to keep it compatible with pure shell https://github.com/sindresorhus/pure/blob/47c0c881f0e7cfdb5eaccd335f52ad17b897c060/pure.zsh#L235
      unset builder out shellHook stdenv system
      # Flakes stuff
      unset dontAddDisableDepTrack outputs

      # For `nix develop`. We get /noshell on Linux and /sbin/nologin on macOS.
      if [[ "$SHELL" == "/noshell" || "$SHELL" == "/sbin/nologin" ]]; then
        export SHELL=${bashPath}
      fi

      # https://github.com/numtide/devshell/issues/158
      PATH=''${PATH#/path-not-set:}

      export PATH="${profile}/bin:$PATH"

      ${startupEnv}

      ${shellHook}
    '';
  };
in
(derivation derivationArg) // {
  inherit meta passthru;

  # https://github.com/NixOS/nixpkgs/blob/41f7e338216fd7f5e57817c4f8e148d42fb88b24/pkgs/stdenv/generic/make-derivation.nix#L486-L504
  inputDerivation = derivation (derivationArg // {
    # Add a name in case the original drv didn't have one
    name = derivationArg.name or "inputDerivation";
    # This always only has one output
    outputs = [ "out" ];

    # Propagate the original builder and arguments, since we override
    # them and they might contain references to build inputs
    _derivation_original_builder = derivationArg.builder;
    _derivation_original_args = derivationArg.args;

    builder = bashPath;
    # The bash builtin `export` dumps all current environment variables,
    # which is where all build input references end up (e.g. $PATH for
    # binaries). By writing this to $out, Nix can find and register
    # them as runtime dependencies (since Nix greps for store paths
    # through $out to find them)
    args = [ "-c" "export > $out" ];
  });
} // passthru
