# using copied code from https://github.com/numtide/devshell
#
# MIT License

# Copyright (c) 2021 Numtide and contributors

# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:

# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.

# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.

{ bashInteractive
, coreutils
, system
, writeTextFile
, pkgs
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
      unset TEMP TEMPDIR TMP ${lib.optionalString (!pkgs.stdenv.isDarwin) "TMPDIR"}
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

      export DEVENV_PROFILE=${profile}

      # add installed packages to PATH
      export PATH="$DEVENV_PROFILE/bin:$PATH"

      # prepend common compilation lookup paths
      export PKG_CONFIG_PATH="$DEVENV_PROFILE/lib/pkgconfig:$PKG_CONFIG_PATH"
      export LD_LIBRARY_PATH="$DEVENV_PROFILE/lib:$LD_LIBRARY_PATH"
      export LIBRARY_PATH="$DEVENV_PROFILE/lib:$LIBRARY_PATH"
      export C_INCLUDE_PATH="$DEVENV_PROFILE/include:$C_INCLUDE_PATH"

      # these provide shell completions / default config options
      export XDG_DATA_DIRS="$DEVENV_PROFILE/share:$XDG_DATA_DIRS"
      export XDG_CONFIG_DIRS="$DEVENV_PROFILE/etc/xdg:$XDG_CONFIG_DIRS"

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
