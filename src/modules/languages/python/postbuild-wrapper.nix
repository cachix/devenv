{ lib, jq }:

# Modified from the upstream post-build patch.
# https://github.com/NixOS/nixpkgs/blob/4c5533c55af2c3899fa4696e26430ef567601dad/pkgs/development/interpreters/python/wrapper.nix
#
# devenv-specific modifications:
#
# - Use jq to extract buildEnv `paths` and re-process them
# - Use --resolve-argv0 for the main python executable wrapper.
#   This correctly points to the wrapped env when the executable is a symlink, e.g. in a devenv profile.
{
  # Python derivation info
  python
, # Wrapper configuration
  permitUserSite ? false
, makeWrapperArgs ? [ ]
,
}:

let
  pythonExecutable = "${placeholder "out"}/bin/${python.executable}";
  pythonPath = "${placeholder "out"}/${python.sitePackages}";
in
''
  # Extract paths from the buildEnv metadata.
  # With __structuredAttrs (newer nixpkgs), all attributes live in
  # $NIX_ATTRS_JSON_FILE under "chosenOutputs". Older nixpkgs exposed
  # them as $pkgsPath or $pkgs shell variables.
  if [ -n "''${NIX_ATTRS_JSON_FILE:-}" ]; then
    paths=$(${lib.getExe jq} -r '.chosenOutputs[].paths[]' "$NIX_ATTRS_JSON_FILE")
  elif [ -n "''${pkgsPath:-}" ]; then
    paths=$(${lib.getExe jq} -r '.[].paths[]' "$pkgsPath")
  else
    paths=$(echo "$pkgs" | ${lib.getExe jq} -r '.[].paths[]')
  fi

  for path in $paths; do
    if [ -d "$path/bin" ]; then
      cd "$path/bin"
      for prg in *; do
        if [ -f "$prg" ] && [ -x "$prg" ]; then
          rm -f "$out/bin/$prg"
          if [ "$prg" = "${python.executable}" ]; then
            makeWrapper "${python.interpreter}" "$out/bin/$prg" \
              --inherit-argv0 \
              --resolve-argv0 \
              ${lib.optionalString (!permitUserSite) ''--set PYTHONNOUSERSITE "true" \''}
              ${lib.concatStringsSep " " makeWrapperArgs}
          elif [ "$(readlink "$prg")" = "${python.executable}" ]; then
            ln -s "${python.executable}" "$out/bin/$prg"
          else
            makeWrapper "$path/bin/$prg" "$out/bin/$prg" \
              --set NIX_PYTHONPREFIX "$out" \
              --set NIX_PYTHONEXECUTABLE ${pythonExecutable} \
              --set NIX_PYTHONPATH ${pythonPath} \
              ${lib.optionalString (!permitUserSite) ''--set PYTHONNOUSERSITE "true" \''}
              ${lib.concatStringsSep " " makeWrapperArgs}
          fi
        fi
      done
    fi
  done
''
