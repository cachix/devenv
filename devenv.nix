{ pkgs, nix, lib, config, ... }:

{
  packages = [
    (import ./src/devenv.nix { inherit pkgs nix; })
    pkgs.python3Packages.mkdocs-material
  ];

  processes.docs.exec = "mkdocs serve";

  enterShell = ''
    echo hola
  '';

  scripts."run-devenv-tests".exec = ''
    set -xe

    pushd examples/simple
      devenv init
    popd

    for dir in $(ls examples); do
      pushd examples/$dir 
      devenv ci
      devenv shell ls
      popd
    done

    # TODO: test direnv integration
    # TODO: test DIRENV_ACTIVE
  '';
  scripts."generate-doc-options".exec = ''
    options=$(nix build --extra-experimental-features 'flakes nix-command' --print-out-paths --no-link '.#devenv-docs-options')
    echo "# devenv.nix options" > docs/reference/options.md
    echo >> docs/reference/options.md
    cat $options >> docs/reference/options.md
  '';
  scripts."generate-languages-example".exec = ''
    cat > examples/all-languages/devenv.nix <<EOF
    { pkgs, ... }: {

      # Enable all languages tooling!
      ${lib.concatStringsSep "\n  " (map (lang: "languages.${lang}.enable = true;") (builtins.attrNames config.languages))}
    
      # If you're missing a language, please contribute it by following examples of other languages <3
    }
    EOF
  '';

  pre-commit.hooks = {
    nixpkgs-fmt.enable = true;
    shellcheck.enable = true;
    #markdownlint.enable = true;
  };
  pre-commit.settings.markdownlint.config = {
    MD013 = {
      line_length = 120;
    };
    MD033 = false;
    MD034 = false;
  };
}
