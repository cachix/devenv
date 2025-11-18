{
  config,
  lib,
  pkgs,
  ...
}:

{
  # Disable browserlist warnings that break git hooks
  env.BROWSERSLIST_IGNORE_OLD_DATA = "1";

  packages = [ pkgs.cairo ];

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
