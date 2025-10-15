{
  config,
  lib,
  pkgs,
  ...
}:

{
  # Disable browserlist warnings that break git hooks
  env.BROWSERSLIST_IGNORE_OLD_DATA = "1";

  git-hooks.hooks = {
    generate-doc-css = {
      enable = true;
      name = "generate-doc-css";
      entry = config.scripts."devenv-generate-doc-css".exec;
      files = "${config.git.root}/docs/src/assets/extra.css";
    };
  };

  languages = {
    # For developing the mkdocs-based documentation
    python = {
      enable = true;
      # Use a faster package manager
      uv.enable = true;
      venv = {
        enable = true;
        requirements = ./requirements.txt;
      };
    };

    # For developing the frontend doc dependencies
    javascript = {
      enable = true;
      directory = "${config.git.root}/docs";
      npm = {
        enable = true;
        install.enable = true;
      };
    };
  };

  # `devenv up` processes to run
  processes = {
    # Serve the mkdocs documentation website with live reload
    docs = {
      exec = "mkdocs serve";
      cwd = config.git.root + "/docs";
    };

    # Watch files for changes to re-compile the Tailwind CSS
    tailwind = {
      exec = "watchexec -e html,css,js devenv-generate-doc-css";
      cwd = config.git.root + "/docs";
    };
  };

  scripts."devenv-generate-doc-css" = {
    description = "Generate CSS for the docs.";
    exec = "${lib.getExe pkgs.tailwindcss} -m -i src/assets/extra.css -o src/assets/output.css";
  };

  scripts."devenv-generate-doc-options" = {
    description = "Generate option docs";
    exec = ./scripts/generate-doc-options.sh;
  };

  scripts."devenv-generate-languages-example" = {
    description = "Generate an example enabling every supported language";
    exec = import ./scripts/generate-languages-example.nix {
      inherit lib;
      inherit (config) languages;
    };
  };
  scripts."devenv-generate-docs" = {
    description = "Generate lists of all languages and services";
    exec = import ./scripts/generate-docs.nix {
      inherit lib;
      inherit (config) languages services;
    };
  };

  scripts."devenv-generate-individual-docs" = {
    description = "Generate individual docs of all devenv modules";
    exec = ./scripts/generate-individual-docs.sh;
  };

  scripts."devenv-verify-individual-docs" = {
    description = "Generate missing template markdown files";
    exec = ./scripts/verify-individual-docs.sh;
  };

  tasks = {
    "devenv:compile-requirements" = {
      before = [ "devenv:python:virtualenv" ];
      exec = "uv pip compile --no-header ${config.git.root}/docs/requirements.in -o ${config.git.root}/docs/requirements.txt";
      execIfModified = [
        "${config.git.root}/docs/requirements.in"
        "${config.git.root}/docs/requirements.txt"
      ];
    };
  };
}
