{ inputs, pkgs, lib, config, ... }:

{
  packages = [
    (import ./src/devenv.nix { inherit pkgs; nix = inputs.nix; })
    pkgs.cairo
    pkgs.yaml2json
  ];

  languages.python.enable = true;
  languages.python.poetry.enable = true;

  devcontainer.enable = true;
  devcontainer.settings.customizations.vscode.extensions = [ "bbenoist.Nix" ];
  difftastic.enable = true;

  # bin/mkdocs serve --config-file mkdocs.insiders.yml
  processes.docs.exec = "mkdocs serve";
  processes.build.exec = "${pkgs.watchexec}/bin/watchexec -e nix nix build";

  scripts.devenv-bump-version.exec = ''
    # TODO: ask for the new version
    # TODO: update the version in the mkdocs.yml
    echo assuming you bumped the version in mkdocs.yml, populating src/modules/latest-version
    cat mkdocs.yml | yaml2json | jq -r '.extra.devenv.version' > src/modules/latest-version
  '';
  scripts.devenv-run-tests.exec = ''
    set -xe

    pushd examples/simple
      # this should fail since files already exist
      devenv init && exit 1
    popd

    tmp="$(mktemp -d)"
    devenv init "$tmp"
    pushd "$tmp"
      devenv ci
    popd
    rm -rf "$tmp"

    # Test devenv integrated into bare Nix flake
    tmp="$(mktemp -d)"
    pushd "$tmp"
      nix flake init --template ''${DEVENV_ROOT}#simple
      nix flake update \
        --override-input devenv ''${DEVENV_ROOT}
      nix develop --command echo nix-develop started succesfully |& tee ./console
      grep -F 'nix-develop started succesfully' <./console
      grep -F "$(${lib.getExe pkgs.hello})" <./console
    popd
    rm -rf "$tmp"

    # Test devenv integrated into flake-parts Nix flake
    tmp="$(mktemp -d)"
    pushd "$tmp"
      nix flake init --template ''${DEVENV_ROOT}#flake-parts
      nix flake update \
        --override-input devenv ''${DEVENV_ROOT}
      nix develop --command echo nix-develop started succesfully |& tee ./console
      grep -F 'nix-develop started succesfully' <./console
      grep -F "$(${lib.getExe pkgs.hello})" <./console
      # Test that a container can be built
      nix build .#container-processes
    popd
    rm -rf "$tmp"

    # TODO: test DIRENV_ACTIVE
  '';
  scripts.devenv-test-all-examples.exec = ''
    for dir in $(ls examples); do
      devenv-test-example $dir
    done
  '';
  scripts.devenv-test-example.exec = ''
    set -e
    pushd examples/$1 
    devenv ci
    if [ -f .test.sh ]
    then
      devenv shell ./.test.sh
    else
      devenv shell ls
    fi
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
