{
  description = "devenv.sh - Fast, Declarative, Reproducible, and Composable Developer Environments";

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  # this needs to be rolling so we're testing what most devs are using
  inputs.nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
  inputs.git-hooks = {
    url = "github:cachix/git-hooks.nix";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
    };
  };
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };
  inputs.flake-parts = {
    url = "github:hercules-ci/flake-parts";
    inputs = {
      nixpkgs-lib.follows = "nixpkgs";
    };
  };
  inputs.nix = {
    url = "github:cachix/nix/devenv-2.30.5";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
      flake-parts.follows = "flake-parts";
      git-hooks-nix.follows = "git-hooks";
      nixpkgs-23-11.follows = "";
      nixpkgs-regression.follows = "";
    };
  };
  inputs.cachix = {
    url = "github:cachix/cachix/latest";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "";
      git-hooks.follows = "git-hooks";
      devenv.follows = "";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      git-hooks,
      nix,
      ...
    }@inputs:
    let
      systems = [
        "x86_64-linux"
        "i686-linux"
        "x86_64-darwin"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        builtins.listToAttrs (
          map (name: {
            inherit name;
            value = f name;
          }) systems
        );
      mkDocOptions =
        {
          pkgs,
          options,
          docOpts ? { },
        }:
        let
          inherit (pkgs) lib;
          sources = [
            {
              name = "${self}";
              url = "https://github.com/cachix/devenv/blob/main";
            }
            {
              name = "${git-hooks}";
              url = "https://github.com/cachix/git-hooks.nix/blob/master";
            }
          ];
          rewriteSource =
            decl:
            let
              prefix = lib.strings.concatStringsSep "/" (lib.lists.take 4 (lib.strings.splitString "/" decl));
              source = lib.lists.findFirst (src: src.name == prefix) { } sources;
              path = lib.strings.removePrefix prefix decl;
              url = "${source.url}${path}";
            in
            {
              name = url;
              url = url;
            };

          filterOptions = import ./filterOptions.nix lib;

          # Apply a filter to process git-hooks options
          filterGitHooks =
            path: opt:
            # Test if path starts with "git-hooks.hooks"
            if lib.lists.hasPrefix [ "git-hooks" "hooks" ] path then
              # Document the generic submodule options: git-hooks.hooks.<name>.<option>
              if builtins.elemAt path 2 == "_freeformOptions" then
                true
              else
              # For pre-configured hooks, document certain values, like the settings and description.
              # Importantly, don't document the generic submodule options to avoid cluttering the docs.
              if
                builtins.elem (builtins.elemAt path 3) [
                  "enable"
                  "description"
                  "packageOverrides"
                  "settings"
                ]
              then
                true
              else
                false
            else
              true;

          # Build the docs locally. Querying the narinfos takes too long.
          disableSubstitutes =
            drv:
            drv.overrideAttrs (_: {
              allowSubstitutes = false;
            });

          optionsDoc = pkgs.nixosOptionsDoc (
            {
              options = filterOptions filterGitHooks (builtins.removeAttrs options [ "_module" ]);
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
    in
    {
      packages = forAllSystems (
        system:
        let
          overlays = [
            (final: prev: {
              devenv-nix = inputs.nix.packages.${system}.nix-cli;
              cachix = inputs.cachix.packages.${system}.cachix;
            })
          ];
          pkgs = import nixpkgs { inherit overlays system; };
          workspace = pkgs.callPackage ./workspace.nix { };

          evaluatedModules = pkgs.lib.evalModules {
            modules = [
              ./src/modules/top-level.nix
              # Don't emit version warnings when building docs
              { devenv.warnOnNewVersion = false; }
            ];
            specialArgs = { inherit pkgs inputs; };
          };
          options = mkDocOptions {
            pkgs = pkgs;
            options = evaluatedModules.options;
          };
        in
        {
          inherit (workspace) devenv devenv-tasks devenv-tasks-fast-build;
          default = self.packages.${system}.devenv;
          devenv-docs-options = options.optionsCommonMark;
          devenv-docs-options-json = options.optionsJSON;
          devenv-generate-individual-docs =
            let
              inherit (pkgs) lib;

              generateOptionDocs =
                options:
                mkDocOptions {
                  inherit pkgs options;
                  docOpts = {
                    variablelistId = "options";
                  };
                };

              # Default doc template
              defaultDoc = ''
                [comment]: # (Please add your documentation above this line)

                @AUTOGEN_OPTIONS@
              '';

              # The docs to generate:
              #   - options: the options to generate docs for
              #   - srcDir: where to find existing docs
              #   - outDir: where to write the generated docs
              docs = [
                {
                  options = evaluatedModules.options.languages;
                  srcDir = "./languages";
                  outDir = "$out/docs/individual-docs/supported-languages";
                }
                {
                  options = evaluatedModules.options.services;
                  srcDir = "./services";
                  outDir = "$out/docs/individual-docs/supported-services";
                }
                {
                  options = evaluatedModules.options.process.managers;
                  srcDir = "./process-managers";
                  outDir = "$out/docs/individual-docs/supported-process-managers";
                }
              ];
            in
            pkgs.stdenv.mkDerivation {
              name = "generate-individual-docs";
              src = ./docs/src/individual-docs;
              allowSubstitutes = false;
              buildPhase = ''
                AUTOGEN_NOTICE="[comment]: # (Do not edit this file as it is autogenerated. Go to docs/individual-docs if you want to make edits.)"

                ${lib.concatStringsSep "\n" (
                  lib.map (
                    {
                      options,
                      srcDir,
                      outDir,
                    }:
                    ''
                      mkdir -p ${outDir}

                      ${lib.concatStringsSep "\n" (
                        lib.mapAttrsToList (name: options: ''
                          srcFile=${srcDir}/${name}.md
                          outFile=${outDir}/${name}.md
                          optionsFile=${(generateOptionDocs options).optionsCommonMark}

                          # Create output file with autogen notice
                          echo "$AUTOGEN_NOTICE" > "$outFile"

                          # Append source content or default template
                          if [ -f "$srcFile" ]; then
                            tail -n +1 "$srcFile" >> "$outFile"
                          else
                            echo "${defaultDoc}" >> "$outFile"
                          fi

                          # Process and substitute options in place
                          substituteInPlace "$outFile" --subst-var-by AUTOGEN_OPTIONS "$(
                            echo "## Options"
                            echo
                            sed 's/^## /### /g' "$optionsFile"
                          )"

                        '') options
                      )}
                    ''
                  ) docs
                )}
              '';
            };
        }
      );

      modules = ./src/modules;

      templates =
        let

          flake-parts = {
            path = ./templates/flake-parts;
            description = "A flake with flake-parts, direnv and devenv.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };

          simple = {
            path = ./templates/simple;
            description = "A direnv supported Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };
        in
        {
          inherit simple flake-parts;
          terraform = {
            path = ./templates/terraform;
            description = "A Terraform Nix flake with devenv integration.";
            welcomeText = ''
              # `.devenv` should be added to `.gitignore`
              ```sh
                echo .devenv >> .gitignore
              ```
            '';
          };
          default = simple;
        };

      flakeModule = self.flakeModules.default; # Backwards compatibility
      flakeModules = {
        default = import ./flake-module.nix self;
        readDevenvRoot =
          { inputs, lib, ... }:
          {
            config =
              let
                devenvRootFileContent =
                  if inputs ? devenv-root then builtins.readFile inputs.devenv-root.outPath else "";
              in
              lib.mkIf (devenvRootFileContent != "") {
                devenv.root = devenvRootFileContent;
              };
          };
      };

      lib = {
        mkConfig =
          args@{
            pkgs,
            inputs,
            modules,
          }:
          (self.lib.mkEval args).config;
        mkEval =
          {
            pkgs,
            inputs,
            modules,
          }:
          let
            moduleInputs = {
              inherit git-hooks;
            }
            // inputs;
            project = inputs.nixpkgs.lib.evalModules {
              specialArgs = moduleInputs // {
                inputs = moduleInputs;
              };
              modules = [
                { config._module.args.pkgs = inputs.nixpkgs.lib.mkDefault pkgs; }
                (self.modules + /top-level.nix)
                (
                  { config, ... }:
                  {
                    devenv.warnOnNewVersion = false;
                    devenv.flakesIntegration = true;
                  }
                )
              ]
              ++ modules;
            };
          in
          project;
        mkShell =
          args:
          let
            config = self.lib.mkConfig args;
          in
          config.shell
          // {
            ci = config.ciDerivation;
            inherit config;
          };
      };

      overlays.default = final: prev: {
        devenv = self.packages.${prev.system}.default;
      };
    };
}
