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
  inputs.nix = {
    url = "github:cachix/nix/devenv-2.30";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "flake-compat";
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
    { self
    , nixpkgs
    , git-hooks
    , nix
    , ...
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
          map
            (name: {
              inherit name;
              value = f name;
            })
            systems
        );
      mkPackage =
        pkgs: attrs:
        pkgs.callPackage ./package.nix (
          {
            nix = inputs.nix.packages.${pkgs.stdenv.system}.nix-cli;
            inherit (inputs.cachix.packages.${pkgs.stdenv.system}) cachix;
          }
          // attrs
        );
      mkDevShellPackage = config: pkgs: import ./src/devenv-devShell.nix { inherit config pkgs; };
      mkDocOptions =
        pkgs:
        let
          inherit (pkgs) lib;
          eval = pkgs.lib.evalModules {
            modules = [
              ./src/modules/top-level.nix
              { devenv.warnOnNewVersion = false; }
            ];
            specialArgs = { inherit pkgs inputs; };
          };
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

          options = pkgs.nixosOptionsDoc {
            options = filterOptions filterGitHooks (builtins.removeAttrs eval.options [ "_module" ]);
            transformOptions = opt: (opt // { declarations = map rewriteSource opt.declarations; });
          };
        in
        options;

    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          options = mkDocOptions pkgs;
          evaluatedModules = pkgs.lib.evalModules {
            modules = [
              ./src/modules/top-level.nix
            ];
            specialArgs = { inherit pkgs inputs; };
          };

          optionsDocs =
            optionParameter:
            pkgs.nixosOptionsDoc {
              options = optionParameter;
              variablelistId = "options";
              transformOptions = options: removeAttrs options [ "declarations" ];
            };
        in
        {
          default = self.packages.${system}.devenv;
          devenv = mkPackage pkgs { };
          devenv-tasks = mkPackage pkgs { build_tasks = true; };
          devenv-docs-options = options.optionsCommonMark;
          devenv-docs-options-json = options.optionsJSON;
          devenv-generate-individual-docs =
            let
              inherit (pkgs) lib;
              languageOptions = evaluatedModules.options.languages;
              serviceOptions = evaluatedModules.options.services;
              processManagersOptions = evaluatedModules.options.process.managers;
              processedOptions = option: builtins.mapAttrs (key: options: optionsDocs options) option;
            in
            pkgs.stdenv.mkDerivation {
              name = "generate-individual-docs";
              src = ./docs/individual-docs;
              allowSubstitutes = false;
              buildPhase = ''
                languageDir=./languages
                serviceDir=./services
                processManagerDir=./process-managers
                mkdir -p $out/docs/individual-docs/supported-languages
                mkdir -p $out/docs/individual-docs/supported-services
                mkdir -p $out/docs/individual-docs/supported-process-managers
                AUTOGEN_NOTICE="[comment]: # (Do not edit this file as it is autogenerated. Go to docs/individual-docs if you want to make edits.)"

                ${lib.concatStringsSep "\n" (
                  lib.mapAttrsToList (key: options: ''
                    content=$(cat ${options.optionsCommonMark})
                    # Add Options header and increase heading levels for individual options
                    content="## Options"$'\n\n'"$(echo "$content" | sed 's/^## /### /g')"

                    file=$languageDir/${key}.md

                    sed -i "1i$AUTOGEN_NOTICE" "$file"
                    substituteInPlace $file \
                    --subst-var-by \
                    AUTOGEN_OPTIONS \
                    "$content"

                    cp $file $out/docs/individual-docs/supported-languages/${key}.md

                  '') (processedOptions languageOptions)
                )}

                ${lib.concatStringsSep "\n" (
                  lib.mapAttrsToList (key: options: ''
                    content=$(cat ${options.optionsCommonMark})
                    # Add Options header and increase heading levels for individual options
                    content="## Options"$'\n\n'"$(echo "$content" | sed 's/^## /### /g')"

                    file=$serviceDir/${key}.md

                    sed -i "1i$AUTOGEN_NOTICE" "$file"
                    substituteInPlace $file \
                    --subst-var-by \
                    AUTOGEN_OPTIONS \
                    "$content"

                    cp $file $out/docs/individual-docs/supported-services/${key}.md

                  '') (processedOptions serviceOptions)
                )}

                ${lib.concatStringsSep "\n" (
                  lib.mapAttrsToList (key: options: ''
                    content=$(cat ${options.optionsCommonMark})
                    # Add Options header and increase heading levels for individual options
                    content="## Options"$'\n\n'"$(echo "$content" | sed 's/^## /### /g')"

                    file=$processManagerDir/${key}.md

                    sed -i "1i$AUTOGEN_NOTICE" "$file"
                    substituteInPlace $file \
                    --subst-var-by \
                    AUTOGEN_OPTIONS \
                    "$content"

                    cp $file $out/docs/individual-docs/supported-process-managers/${key}.md
                  '') (processedOptions processManagersOptions)
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
          args@{ inputs
          , modules
          , pkgs ? null
          , system ? null
          ,
          }:
          (self.lib.mkEval args).config;
        mkEval =
          { inputs
          , modules
          , pkgs ? null
          , system ? null
          ,
          }:
          let
            # If the user passed a nixpkg we use it to do the evaluation.
            # By default we take the nixpkgs with which devenv was tested.
            moduleLib = if nixpkgs.lib.hasAttr "nixpkgs" inputs then inputs.nixpkgs.lib else nixpkgs.lib;
            # If the user passed a `pkgs` we used it.
            # By default we take the pkgs which devenv was tested against.
            modulePkgs =
              if pkgs == null then
                assert system != null "System must be specified when no `pkgs` argument is used.";
                moduleLib.legacyPackages.${system} else pkgs;

            moduleInputs = {
              inherit git-hooks;
            } // inputs;

            project = moduleLib.evalModules {
              specialArgs = moduleInputs // {
                inputs = moduleInputs;
              };
              modules = [
                ({ lib, ... }: { config._module.args.pkgs = lib.mkDefault modulePkgs; })
                (self.modules + /top-level.nix)
                (
                  { config, lib, ... }:
                  {
                    packages = lib.mkBefore [
                      (mkDevShellPackage config pkgs)
                    ];
                    devenv.warnOnNewVersion = false;
                    devenv.flakesIntegration = true;
                  }
                )
              ] ++ modules;
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
