{ lib
, bashInteractive
, buildEnv
, coreutils
, runCommand
, stdenv
, system
, writeTextFile
}:
let
  join = lib.concatStringsSep;

  bashPath = "${bashInteractive}/bin/bash";

  drvOrPackageToPaths = drvOrPackage:
    if drvOrPackage ? outputs then
      map (output: drvOrPackage.${output}) drvOrPackage.outputs
    else
      [ drvOrPackage ];

  envToBash = name: value: "export ${name}=${lib.escapeShellArg (toString value)}";

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

  baseFHS = runCommand "base-fhs" { } ''
    mkdir -p $out/{bin,etc,share}
  '';
in
{ name ? "devenv"
, env ? { }
, packages ? [ ]
, shellHook ? ""
, meta ? { }
, passthru ? { }
} @ args:
let
  # Entrypoint script
  # TODO: there is more to do here
  binDevenv = writeTextFile {
    name = "${name}-bin-devenv";
    destination = "/bin/devenv";
    executable = true;
    text = ''
      #!${bashPath}
      # Devenv entrypoint
      set -euo pipefail

      exec ${bashPath} --profile "$(dirname "$0")/etc/profile" "$@"
    '';
  };

  # Setting up environment variables
  etcProfile = writeTextFile {
    name = "${name}-etc-profile";
    destination = "/etc/profile";
    text = ''
      # Set env vars
      ${join "\n" (lib.mapAttrsToList envToBash env)}

      # Point to the profile root
      export DEVENV_PROFILE=@devenv_profile@

      # Add installed packages to PATH
      export PATH="$DEVENV_PROFILE/bin:$PATH"

      # Prepend common compilation lookup paths
      export PKG_CONFIG_PATH="$DEVENV_PROFILE/lib/pkgconfig:''${PKG_CONFIG_PATH-}"
      export LD_LIBRARY_PATH="$DEVENV_PROFILE/lib:''${LD_LIBRARY_PATH-}"
      export LIBRARY_PATH="$DEVENV_PROFILE/lib:''${LIBRARY_PATH-}"
      export C_INCLUDE_PATH="$DEVENV_PROFILE/include:''${C_INCLUDE_PATH-}"

      # These provide shell completions / default config options
      export XDG_DATA_DIRS="$DEVENV_PROFILE/share:''${XDG_DATA_DIRS-}"
      export XDG_CONFIG_DIRS="$DEVENV_PROFILE/etc/xdg:''${XDG_CONFIG_DIRS-}"

      ${shellHook}
    '';
  };

  # Merge everything together
  profile = buildEnv {
    name = "${name}-profile";
    paths = [ baseFHS binDevenv etcProfile ] ++ (lib.flatten (map drvOrPackageToPaths packages));
    ignoreCollisions = true;
    postBuild = ''
      # Fix the path to the profile root
      rm $out/etc/profile
      sed "s|@devenv_profile@|$out|g" < ${etcProfile}/etc/profile > $out/etc/profile
    '';
  };

  passthru = (args.passthru or { }) // { inherit profile; };
  meta = { mainProgram = "devenv"; } // (args.meta or { });

  # Create a naked shell for the `nix-shell`
  derivationArg = {
    inherit name system;

    # `nix develop` actually checks and uses builder. And it must be bash.
    builder = bashPath;

    # Bring in the dependencies on `nix-build`
    args = [
      "-ec"
      "${coreutils}/bin/ln -s ${profile} $out; exit 0"
    ];

    # $stdenv/setup is loaded by nix-shell during startup.
    # https://github.com/nixos/nix/blob/377345e26f1ac4bbc87bb21debcc52a1d03230aa/src/nix-build/nix-build.cc#L429-L432
    stdenv = stdenv;

    # The shellHook is loaded directly by `nix develop`. But nix-shell
    # requires that other trampoline.
    shellHook = ''
      # Remove all the unnecessary noise that is set by the build env
      unset NIX_BUILD_TOP NIX_BUILD_CORES NIX_STORE
      unset TEMP TEMPDIR TMP ${lib.optionalString (!stdenv.isDarwin) "TMPDIR"}
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

      # Load the devenv profile
      source ${profile}/etc/profile
    '';
  };
in
(derivation derivationArg) // { inherit meta passthru; } // passthru
