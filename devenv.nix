{ pkgs, ... }:

{
  packages = [
    (import ./src/devenv.nix { inherit pkgs; })
    pkgs.python3Packages.mkdocs-material
  ];

  processes.docs.exec = "mkdocs serve";

  enterShell = ''
    echo hola
  '';

  scripts."run-devenv-tests".exec = ''
    set -xe

    for dir in $(ls examples); do
      pushd examples/$dir 
      devenv ci
      devenv shell
      exit
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

  pre-commit.hooks = {
    nixpkgs-fmt.enable = true;
    shellcheck.enable = true;
    markdownlint.enable = true;
  };
  pre-commit.settings.markdownlint.config = {
    MD013 = {
      line_length = 120;
    };
    MD033 = false;
    MD034 = false;
  };
}
