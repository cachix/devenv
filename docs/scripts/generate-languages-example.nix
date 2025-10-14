{ lib, languages }:

''
  cat > examples/supported-languages/devenv.nix <<EOF
  # DO NOT MODIFY.
  # This file was generated bu devenv-generate-languages-example.
  { pkgs, ... }: {

    # Enable all languages tooling!
    ${lib.concatStringsSep "\n  " (
      map (lang: "languages.${lang}.enable = true;") (builtins.attrNames languages)
    )}

    # If you're missing a language, please contribute it by following examples of other languages <3
  }
  EOF
''
