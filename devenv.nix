{ inputs, pkgs, lib, config, ... }: {
  env.DEVENV_NIX = inputs.nix.packages.${pkgs.stdenv.system}.nix;

  packages = [
    pkgs.cairo
    pkgs.git
    pkgs.xorg.libxcb
    pkgs.yaml2json
    pkgs.tesh
    pkgs.watchexec
    pkgs.openssl
  ] ++ lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk; [
    frameworks.SystemConfiguration
  ]);

  languages.nix.enable = true;
  # for cli
  languages.rust.enable = true;
  # for docs
  languages.python.enable = true;
  # it breaks glibc
  languages.python.manylinux.enable = false;
  # speed it up
  languages.python.uv.enable = true;
  languages.python.venv.enable = true;
  languages.python.venv.requirements = ./requirements.txt;
  languages.javascript.enable = true;
  languages.javascript.npm.enable = true;
  languages.javascript.npm.install.enable = true;

  devcontainer.enable = true;
  devcontainer.settings.customizations.vscode.extensions = [ "jnoortheen.nix-ide" ];
  difftastic.enable = true;

  processes = {
    docs.exec = "mkdocs serve";
    tailwind.exec = "watchexec -e html,css,js devenv-generate-doc-css";
  };

  scripts.devenv-test-cli = {
    description = "Test devenv CLI.";
    exec = ''
      set -xe
      set -o pipefail

      pushd examples/simple
        # this should fail since files already exist
        devenv init && exit 1
      popd

      tmp="$(mktemp -d)"
      devenv init "$tmp"
      pushd "$tmp"
        devenv version
        devenv --override-input devenv path:${config.devenv.root}?dir=src/modules test
      popd
      rm -rf "$tmp"

      # Test devenv integrated into bare Nix flake
      tmp="$(mktemp -d)"
      pushd "$tmp"
        nix flake init --template ''${DEVENV_ROOT}#simple
        nix flake update \
          --override-input devenv ''${DEVENV_ROOT}
        nix develop --accept-flake-config --no-pure-eval --command echo nix-develop started succesfully |& tee ./console
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
        nix develop --accept-flake-config --override-input devenv-root "file+file://"<(printf %s "$PWD") --command echo nix-develop started succesfully |& tee ./console
        grep -F 'nix-develop started succesfully' <./console
        grep -F "$(${lib.getExe pkgs.hello})" <./console
        # Test that a container can be built
        if $(uname) == "Linux"
        then
          nix build --override-input devenv-root "file+file://"<(printf %s "$PWD") --accept-flake-config --show-trace .#container-processes
        fi
      popd
      rm -rf "$tmp"
    '';
  };
  scripts."devenv-generate-doc-css" = {
    description = "Generate CSS for the docs.";
    exec = ''
      ${lib.getExe pkgs.tailwindcss} build -i docs/assets/extra.css -o docs/assets/output.css
    '';
  };
  scripts."devenv-generate-doc-options" = {
    description = "Generate option docs.";
    exec = ''
      set -e
      output_file=docs/reference/options.md
      options=$(nix build --accept-flake-config --no-pure-eval --extra-experimental-features 'flakes nix-command' --show-trace --print-out-paths --no-link '.#devenv-docs-options')
      echo "# devenv.nix options" > $output_file
      echo >> $output_file
      cat $options >> $output_file
      # https://github.com/NixOS/nixpkgs/issues/224661
      sed -i 's/\\\././g' $output_file
    '';
  };
  scripts."devenv-generate-languages-example" = {
    description = "Generate an example enabling every supported language.";
    exec = ''
      cat > examples/supported-languages/devenv.nix <<EOF
      { pkgs, ... }: {

        # Enable all languages tooling!
        ${lib.concatStringsSep "\n  " (map (lang: "languages.${lang}.enable = true;") (builtins.attrNames config.languages))}

        # If you're missing a language, please contribute it by following examples of other languages <3
      }
      EOF
    '';
  };
  scripts."devenv-generate-docs" = {
    description = "Generate lists of all languages and services.";
    exec = ''
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
  };

  pre-commit.hooks = {
    nixpkgs-fmt.enable = true;
    #shellcheck.enable = true;
    #clippy.enable = true;
    rustfmt.enable = true;
    #markdownlint.enable = true;
    markdownlint.settings.configuration = {
      MD013 = {
        line_length = 120;
      };
      MD033 = false;
      MD034 = false;
    };
    generate-doc-css = {
      enable = true;
      name = "generate-doc-css";
      # Copied from devenv-generate-doc-css
      # In CI, the auto-commit action doesn't run in the shell, so it can't reuse our scripts.
      # And the following command is curently too slow to be a pre-commit command.
      # entry = "devenv shell devenv-generate-doc-css";
      entry = "${lib.getExe pkgs.tailwindcss} build -i docs/assets/extra.css -o docs/assets/output.css";
    };
  };
}
