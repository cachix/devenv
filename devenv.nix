{ pkgs, nix, lib, config, ... }:

{
  packages = [
    (import ./src/devenv.nix { inherit pkgs nix; })
    pkgs.python3Packages.virtualenv
    pkgs.python3Packages.cairocffi
    pkgs.yaml2json
  ];

  # bin/mkdocs serve --config-file mkdocs.insiders.yml
  processes.docs.exec = "bin/mkdocs serve";
  processes.build.exec = "${pkgs.watchexec}/bin/watchexec -e nix nix build";

  enterShell = ''
    echo "To Install:"
    echo
    echo "virtualenv ."
    echo "bin/pip install -r requirements.txt"
  '';

  scripts.bump-version.exec = ''
    echo assuming you bumped the version in mkdocs.yml, populating src/version
    cat mkdocs.yml | yaml2json | jq '.extra.devenv.version' > src/version
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
    cat > examples/supported-languages/devenv.nix <<EOF
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
