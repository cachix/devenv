{ pkgs
, lib
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
      outDir = "$out/languages";
    }
    {
      options = options.services;
      outDir = "$out/services";
    }
    {
      options = options.process.managers;
      outDir = "$out/supported-process-managers";
    }
  ];

  # Generate standalone option docs (one file per module)
  generateOptionDocs = pkgs.stdenv.mkDerivation {
    name = "generate-option-docs";
    allowSubstitutes = false;
    dontUnpack = true;
    buildPhase = ''
      ${lib.concatStringsSep "\n" (
        lib.map (
          { options, outDir }:
          ''
            mkdir -p ${outDir}

            ${lib.concatStringsSep "\n" (
              lib.mapAttrsToList (name: opts:
                let optDoc = mkModuleOptionDocs opts; in
                ''
                  {
                    echo "## Options"
                    echo
                    sed 's/^## /### /g' "${optDoc.optionsCommonMark}"
                  } > ${outDir}/${name}-options.md
                ''
              ) options
            )}
          ''
        ) docs
      )}
    '';
    installPhase = ''
      mkdir -p $out
      cp -r . $out/
    '';
  };

in
{
  devenv.warnOnNewVersion = false;

  packages = [ pkgs.jq ];

  # Expose the outputs for the flake and scripts to use
  outputs = {
    devenv-docs-options = allOptions.optionsCommonMark;
    devenv-docs-options-json = allOptions.optionsJSON;
    devenv-generate-option-docs = generateOptionDocs;
  };

  scripts."devenv-build" = {
    description = "Run devenv build, handling JSON output for devenv 2.0.0+";
    exec = ./scripts/devenv-build.sh;
  };

  scripts."devenv-generate-doc-options" = {
    description = "Generate option docs";
    exec = ./scripts/generate-doc-options.sh;
  };

  scripts."devenv-generate-option-docs" = {
    description = "Generate option docs for all devenv modules";
    exec = ./scripts/generate-option-docs.sh;
  };

  scripts."devenv-verify-module-docs" = {
    description = "Generate missing module doc files";
    exec = ./scripts/verify-module-docs.sh;
  };

  scripts."devenv-generate-docs" = {
    description = "Generate lists of all languages and services";
    exec = import ./scripts/generate-docs.nix {
      inherit lib;
      inherit (options) languages services;
    };
  };
}
