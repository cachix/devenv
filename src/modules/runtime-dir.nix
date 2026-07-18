{
  # Resolve the runtime directory without consulting ambient state directly,
  # so the flakes-integration fallback can be kept in lockstep with the Rust
  # resolver and tested with synthetic environments.
  resolve =
    { dotfile
    , uid
    , getEnv ? builtins.getEnv
    , pathExists ? builtins.pathExists
    ,
    }:
    let
      hashedDotfile = builtins.hashString "sha256" (toString dotfile);
      shortHash = builtins.substring 0 7 hashedDotfile;
      plainName = "devenv-${shortHash}";
      uidName = "devenv-${uid}-${shortHash}";

      inherited = getEnv "DEVENV_RUNTIME";
      inheritedName = baseNameOf inherited;
      inheritedMatches =
        builtins.match "devenv-([0-9]+-)?${shortHash}" inheritedName != null
        && pathExists inherited;

      xdg = getEnv "XDG_RUNTIME_DIR";
      tmpdir = getEnv "TMPDIR";
      legacy = "${tmpdir}/${plainName}";
      legacyManagerExists =
        tmpdir != ""
        && pathExists "${legacy}/processes/native.sock";

      runUser = "/run/user/${uid}";
    in
    if inheritedMatches then
      inherited
    else if xdg != "" then
      "${xdg}/${plainName}"
    else if legacyManagerExists then
      legacy
    else if pathExists runUser then
      "${runUser}/${plainName}"
    else
      "/tmp/${uidName}";

  # Render the shell hook fragment that creates and validates the resolved
  # directory. coreutils is injected to make the security-sensitive commands
  # explicit and to keep the fragment directly testable.
  prepare =
    { coreutils
    , runtime
    ,
    }:
    let
      escapedRuntime =
        "'${builtins.replaceStrings [ "'" ] [ "'\"'\"'" ] (toString runtime)}'";
    in
    ''
      devenv_runtime_path=${escapedRuntime}
      if [ -L "$devenv_runtime_path" ]; then
        echo "devenv: refusing symlinked runtime directory $devenv_runtime_path" >&2
        return 1 2>/dev/null || exit 1
      fi
      if ! ${coreutils}/bin/mkdir -p -m 700 -- "$devenv_runtime_path"; then
        echo "devenv: failed to create runtime directory $devenv_runtime_path" >&2
        return 1 2>/dev/null || exit 1
      fi
      if [ -L "$devenv_runtime_path" ] || [ ! -d "$devenv_runtime_path" ]; then
        echo "devenv: refusing unsafe runtime directory $devenv_runtime_path" >&2
        return 1 2>/dev/null || exit 1
      fi
      devenv_runtime_owner="$(${coreutils}/bin/stat --format=%u -- "$devenv_runtime_path")" || {
        echo "devenv: failed to inspect runtime directory $devenv_runtime_path" >&2
        return 1 2>/dev/null || exit 1
      }
      devenv_current_uid="$(${coreutils}/bin/id -u)" || {
        echo "devenv: failed to determine the current uid" >&2
        return 1 2>/dev/null || exit 1
      }
      if [ "$devenv_runtime_owner" != "$devenv_current_uid" ]; then
        echo "devenv: refusing runtime directory $devenv_runtime_path owned by uid $devenv_runtime_owner" >&2
        return 1 2>/dev/null || exit 1
      fi
      if ! ${coreutils}/bin/chmod --no-dereference 700 -- "$devenv_runtime_path"; then
        echo "devenv: failed to restrict runtime directory $devenv_runtime_path" >&2
        return 1 2>/dev/null || exit 1
      fi
      unset devenv_runtime_path devenv_runtime_owner devenv_current_uid
    '';
}
