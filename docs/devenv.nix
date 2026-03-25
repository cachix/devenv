{ config
, lib
, pkgs
, inputs
, options
, ...
}:

let
  # Import the filterOptions function
  filterOptions = import ./filterOptions.nix lib;

  # Filter options for documentation
  filterGitHooks =
    path: opt:
    if lib.lists.hasPrefix [ "git-hooks" "hooks" ] path then
      builtins.elemAt path 2 == "_freeformOptions"
      || builtins.elem (builtins.elemAt path 3) [
        "enable"
        "packageOverrides"
        "settings"
      ]
    else
      true;

  # Exclude repeated module options from treefmt programs, keep enable and settings
  filterTreefmt =
    path: opt:
    if lib.lists.hasPrefix [ "treefmt" "config" "programs" ] path
      && builtins.length path > 4
    then
      !builtins.elem (builtins.elemAt path 4) [
        "description"
        "excludes"
        "finalPackage"
        "includes"
        "package"
        "priority"
      ]
    else
      true;

  filterDocOptions =
    path: opt:
    filterGitHooks path opt && filterTreefmt path opt;

  getStorePath =
    p:
    lib.pipe p [
      (lib.strings.splitString "/")
      (lib.lists.take 4)
      (lib.strings.concatStringsSep "/")
    ];

  # Rewrite source declarations to GitHub URLs
  sources = [
    {
      name = getStorePath inputs.devenv.outPath;
      url = "https://github.com/cachix/devenv/blob/main";
    }
    {
      name = inputs.git-hooks.outPath;
      url = "https://github.com/cachix/git-hooks.nix/blob/master";
    }
    {
      name = inputs.treefmt-nix.outPath;
      url = "https://github.com/numtide/treefmt-nix/blob/main";
    }
  ];

  rewriteSource =
    decl:
    let
      prefix = getStorePath decl;
      source = lib.lists.findFirst (src: src.name == prefix) { } sources;
      path = lib.strings.removePrefix prefix decl;
      sourceUrl = source.url or (throw "Failed to rewrite source url for module: ${decl}");
      url = sourceUrl + path;
    in
    {
      name = url;
      inherit url;
    };

  # Speed up doc builds by skipping narinfo queries
  disableSubstitutes =
    drv:
    drv.overrideAttrs (_: {
      allowSubstitutes = false;
    });

  mkDocOptions =
    { opts
    , docOpts ? { }
    ,
    }:
    let
      optionsDoc = pkgs.nixosOptionsDoc (
        {
          options = filterOptions filterDocOptions (builtins.removeAttrs opts [ "_module" ]);
          warningsAreErrors = true;
          transformOptions = opt: (opt // { declarations = map rewriteSource opt.declarations; });
        }
        // docOpts
      );
    in
    optionsDoc
    // {
      optionsAsciiDoc = disableSubstitutes optionsDoc.optionsAsciiDoc;
      optionsJSON = disableSubstitutes optionsDoc.optionsJSON;
      optionsCommonMark = disableSubstitutes optionsDoc.optionsCommonMark;
    };

  # Generate documentation for all options
  allOptions = mkDocOptions {
    opts = options;
  };

  # Generate option docs for a set of module options
  mkModuleOptionDocs =
    opts:
    mkDocOptions {
      inherit opts;
      docOpts = {
        variablelistId = "options";
      };
    };

  # The module categories to generate option docs for
  docs = [
    {
      options = options.languages;
      outDir = "languages";
    }
    {
      options = options.services;
      outDir = "services";
    }
    {
      options = options.process.managers;
      outDir = "supported-process-managers";
    }
  ];

  # Generate all doc content into a single derivation
  generatedDocs = pkgs.runCommand "devenv-generated-docs" { allowSubstitutes = false; } ''
    mkdir -p $out/reference

    # Generate the full options reference
    {
      echo "# devenv.nix"
      echo
      cat "${allOptions.optionsCommonMark}"
    } > $out/reference/options.md

    # https://github.com/NixOS/nixpkgs/issues/224661
    sed -i 's/\\\././g' $out/reference/options.md

    # Generate per-module option docs
    ${lib.concatStringsSep "\n" (
      lib.map (
        { options, outDir }:
        ''
          mkdir -p $out/${outDir} $out/stubs/${outDir}

          ${lib.concatStringsSep "\n" (
            lib.mapAttrsToList (name: opts:
              let optDoc = mkModuleOptionDocs opts; in
              ''
                {
                  echo "## Options"
                  echo
                  sed 's/^## /### /g' "${optDoc.optionsCommonMark}"
                } > $out/${outDir}/${name}-options.md

                echo '--8<-- "_generated/${outDir}/${name}-options.md"' > $out/stubs/${outDir}/${name}.md
              ''
            ) options
          )}
        ''
      ) docs
    )}
  '';

in
{
  # Disable browserlist warnings that break git hooks
  env.BROWSERSLIST_IGNORE_OLD_DATA = "1";

  packages = [ pkgs.cairo pkgs.jq ];

  # Expose the outputs for the flake and scripts to use
  outputs = {
    devenv-docs-options-json = allOptions.optionsJSON;
    devenv-generated-docs = generatedDocs;
  };

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

  processes = {
    docs = {
      # Serve the mkdocs documentation website with live reload
      exec = "mkdocs serve";
      cwd = config.git.root + "/docs";
      after = [];
    };
  };

  scripts."devenv-generate-doc-css" = {
    description = "Generate CSS for the docs.";
    exec = "${lib.getExe pkgs.tailwindcss} -m -i src/assets/extra.css -o src/assets/output.css";
  };

  scripts."devenv-build" = {
    description = "Run devenv build, handling JSON output for devenv 2.0.0+";
    exec = ./scripts/devenv-build.sh;
  };

  scripts."devenv-generate-docs" = {
    description = "Generate all option docs";
    exec = ./scripts/generate-docs.sh;
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
    "docs:generate-badge" = {
      exec = "node ${config.git.root}/docs/src/assets/generate-badge.mjs";
    };
    "docs:generate-options" = {
      exec = "devenv-generate-docs";
      before = [ "devenv:processes:docs" ];
    };
    "docs:build" = {
      exec = "mkdocs build";
      cwd = config.git.root + "/docs";
      after = [ "docs:generate-options" "devenv:python:virtualenv" ];
    };
  };
}
