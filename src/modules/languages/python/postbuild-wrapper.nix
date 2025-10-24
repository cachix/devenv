{ lib }:

{
  # Python derivation info
  python,

  # Wrapper configuration
  permitUserSite,
  makeWrapperArgs,
}:

let
  pythonExecutable = "$out/bin/${python.executable}";
  pythonPath = "$out/${python.sitePackages}";
in
''
  for path in $paths; do
    if [ -d "$path/bin" ]; then
      cd "$path/bin"
      for prg in *; do
        if [ -f "$prg" ] && [ -x "$prg" ]; then
          rm -f "$out/bin/$prg"
          if [ "$prg" = "${python.executable}" ]; then
            makeWrapper "${python.interpreter}" "$out/bin/$prg" \
              --inherit-argv0 \
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
