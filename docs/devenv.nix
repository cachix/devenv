{ config
, pkgs
, ...
}:

let
  docsRoot = "${config.devenv.root}/docs";
in
{
  packages = [ pkgs.cairo.out ];

  git-hooks.hooks = {
    generate-doc-css = {
      enable = true;
      name = "generate-doc-css";
      entry = config.scripts."devenv-generate-doc-css".exec;
      files = "^docs/src/assets/extra\\.css$";
      pass_filenames = false;
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
      directory = docsRoot;
      npm = {
        enable = true;
        install.enable = true;
      };
    };
  };

  processes = {
    docs = {
      # Serve the mkdocs documentation website with live reload
      exec = "mkdocs serve";
      cwd = docsRoot;
    };
  };

  scripts."devenv-generate-doc-css" = {
    description = "Generate CSS for the docs.";
    exec = ''
      cd "${docsRoot}" \
        && BROWSERSLIST_IGNORE_OLD_DATA=1 \
          ./node_modules/.bin/tailwindcss -m -i src/assets/extra.css -o src/assets/output.css
    '';
  };

  scripts."docs-sitemap" = {
    description = "Generate a complete docs sitemap (pages + heading anchors) from the running docs server.";
    exec = ''python3 "${docsRoot}/gen/sitemap_with_anchors.py" "$@"'';
  };

  scripts."docs-sitemap-diff" = {
    description = "Diff two docs sitemaps (old vs new) and propose _redirects entries.";
    exec = ''python3 "${docsRoot}/gen/sitemap_diff.py" "$@"'';
  };

  tasks = {
    "devenv:compile-requirements" = {
      before = [ "devenv:python:virtualenv" ];
      exec = "uv pip compile --no-header ${docsRoot}/requirements.in -o ${docsRoot}/requirements.txt";
      execIfModified = [
        "${docsRoot}/requirements.in"
        "${docsRoot}/requirements.txt"
      ];
    };
    "docs:generate-badge" = {
      exec = "node ${docsRoot}/src/assets/generate-badge.mjs";
    };
  };
}
