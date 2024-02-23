{ inputs, pkgs, lib, config, ... }:

{
  env.DEVENV_NIX = inputs.nix.packages.${pkgs.stdenv.system}.nix;

  packages = [
    pkgs.cairo
    pkgs.xorg.libxcb
    pkgs.yaml2json
    pkgs.tesh
    pkgs.python3Packages.black
  ];

  languages.nix.enable = true;
  languages.python.enable = true;
  languages.python.poetry.enable = true;
  languages.python.poetry.install.installRootPackage = true;
  languages.python.poetry.install.groups = [ "docs" ];

  devcontainer.enable = true;
  devcontainer.settings.customizations.vscode.extensions = [ "jnoortheen.nix-ide" ];
  difftastic.enable = true;

  dotenv.enable = true;

  processes.docs.exec = "mkdocs serve";

  scripts.devenv-bump-version.exec = ''
    # TODO: ask for the new version
    # TODO: update the version in the mkdocs.yml
    echo assuming you bumped the version in mkdocs.yml, populating src/modules/latest-version
    cat mkdocs.yml | yaml2json | jq -r '.extra.devenv.version' > src/modules/latest-version
  '';
  scripts.devenv-run-tests.exec = ''
    set -xe
    set -o pipefail

    pushd examples/simple
      # this should fail since files already exist
      devenv init && exit 1
    popd

    tmp="$(mktemp -d)"
    devenv init "$tmp"
    pushd "$tmp"
      echo -e "  devenv:\n    url: path:${config.devenv.root}?dir=src/modules" >> devenv.yaml
      cat devenv.yaml
      devenv version
      devenv ci
      devenv test
    popd
    rm -rf "$tmp"

    # Test devenv integrated into bare Nix flake
    tmp="$(mktemp -d)"
    pushd "$tmp"
      nix flake init --template ''${DEVENV_ROOT}#simple
      nix flake update \
        --override-input devenv ''${DEVENV_ROOT}
      nix develop --accept-flake-config --impure --command echo nix-develop started succesfully |& tee ./console
      grep -F 'nix-develop started succesfully' <./console
      grep -F "$(${lib.getExe pkgs.hello})" <./console

      # Assert that nix-develop fails in pure mode.
      if nix develop --command echo nix-develop started in pure mode |& tee ./console
      then
        echo "nix-develop was able to start in pure mode. This is explicitly not supported at the moment."
        exit 1
      fi
      grep -F 'devenv was not able to determine the current directory.' <./console
    popd
    rm -rf "$tmp"

    # Test devenv integrated into flake-parts Nix flake
    tmp="$(mktemp -d)"
    pushd "$tmp"
      nix flake init --template ''${DEVENV_ROOT}#flake-parts
      nix flake update \
        --override-input devenv ''${DEVENV_ROOT}
      nix develop --accept-flake-config --impure --command echo nix-develop started succesfully |& tee ./console
      grep -F 'nix-develop started succesfully' <./console
      grep -F "$(${lib.getExe pkgs.hello})" <./console
      # Test that a container can be built
      if $(uname) == "Linux"
      then
        nix build --impure --accept-flake-config --show-trace .#container-processes
      fi
    popd
    rm -rf "$tmp"
  '';
  scripts."devenv-generate-doc-options".exec = ''
    set -e
    options=$(nix build --impure --extra-experimental-features 'flakes nix-command' --show-trace --print-out-paths --no-link '.#devenv-docs-options')
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
  scripts."devenv-generate-docs".exec = ''
    cat > docs/services-all.md <<EOF
      \`\`\`nix
      ${lib.concatStringsSep "\n  " (map (lang: "services.${lang}.enable = true;") (builtins.attrNames config.services))}
      \`\`\`
    EOF
    cat > docs/languages-all.md <<EOF
      \`\`\`nix
      ${lib.concatStringsSep "\n  " (map (lang: "languages.${lang}.enable = true;") (builtins.attrNames config.languages))}
      \`\`\`   
    EOF
  '';

  pre-commit.hooks = {
    nixpkgs-fmt.enable = true;
    shellcheck.enable = true;
    black.enable = true;
    #markdownlint.enable = true;
  };
  pre-commit.settings.markdownlint.config = {
    MD013 = {
      line_length = 120;
    };
    MD033 = false;
    MD034 = false;
  };

  tests = config.lib.mkTests ./examples
    // config.lib.mkTests ./tests;
}
