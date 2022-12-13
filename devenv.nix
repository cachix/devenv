{ inputs, pkgs, lib, config, ... }:

{
  packages = [
    (import ./src/devenv.nix { inherit pkgs; nix = inputs.nix; })
    pkgs.python3Packages.virtualenv
    pkgs.python3Packages.cairocffi
    pkgs.yaml2json
  ];

  devcontainer.enable = true;

  # bin/mkdocs serve --config-file mkdocs.insiders.yml
  processes.docs.exec = "bin/mkdocs serve";
  processes.build.exec = "${pkgs.watchexec}/bin/watchexec -e nix nix build";

  enterShell = ''
    echo "To Install:"
    echo
    echo "virtualenv ."
    echo "bin/pip install -r requirements.txt"
  '';

  scripts.devenv-bump-version.exec = ''
    # TODO: ask for the new version
    # TODO: update the version in the mkdocs.yml
    echo assuming you bumped the version in mkdocs.yml, populating src/modules/latest-version
    cat mkdocs.yml | yaml2json | jq '.extra.devenv.version' > src/modules/latest-version
  '';
  scripts.devenv-run-tests.exec = ''
    set -xe

    pushd examples/simple
      # this should fail since files already exist
      devenv init && exit 1
      rm devenv.nix devenv.yaml .envrc
      devenv init
    popd

    # Test devenv integrated into Nix flake
    tmp="$(mktemp -d)"
    pushd "$tmp"
      nix flake init --template ''${DEVENV_ROOT}#simple
      nix flake update \
        --override-input devenv ''${DEVENV_ROOT}
      nix develop --command echo nix-develop started succesfully
    popd
    rm -rf "$tmp"

    # TODO: test direnv integration
    # TODO: test DIRENV_ACTIVE
  '';
  scripts.devenv-test-all-examples.exec = ''
    for dir in $(ls examples); do
      devenv-run-example-test $dir
    done
  '';
  scripts.devenv-test-example.exec = ''
    pushd examples/$1 
    devenv ci
    devenv shell ls
    popd
  '';
  scripts."devenv-generate-doc-options".exec = ''
    set -e
    options=$(nix build --extra-experimental-features 'flakes nix-command' --show-trace --print-out-paths --no-link '.#devenv-docs-options')
    echo "# devenv.nix options" > docs/reference/options.md
    echo >> docs/reference/options.md
    cat $options >> docs/reference/options.md
  '';
  scripts."devenv-generate-languages-example".exec = ''
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
