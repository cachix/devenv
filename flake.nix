{
  description = "devenv.sh - Fast, Declarative, Reproducible, and Composable Developer Environments";

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  inputs.nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
  inputs.git-hooks = {
    url = "github:cachix/git-hooks.nix";
    inputs = {
      nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "";
    };
  };
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };
  inputs.nix = {
    url = "github:domenkozar/nix/devenv-2.24";
    inputs = {
      # disabled until we fix https://github.com/cachix/devenv-nixpkgs/issues/2
      # nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "";
      pre-commit-hooks.follows = "";
      nixpkgs-23-11.follows = "";
      nixpkgs-regression.follows = "";
    };
  };
  inputs.cachix = {
    url = "github:cachix/cachix/latest";
    inputs = {
      # needs hnix-store-nar
      # nixpkgs.follows = "nixpkgs";
      flake-compat.follows = "";
      git-hooks.follows = "";
      devenv.follows = "";
    };
  };

  outputs = { self, nixpkgs, git-hooks, nix, ... }@inputs:
    let
      systems = [ "x86_64-linux" "i686-linux" "x86_64-darwin" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = f: builtins.listToAttrs (map (name: { inherit name; value = f name; }) systems);
      mkPackage = pkgs: pkgs.callPackage ./package.nix {
        inherit (inputs.nix.packages.${pkgs.stdenv.system}) nix;
        inherit (inputs.cachix.packages.${pkgs.stdenv.system}) cachix;
      };
      mkDevShellPackage = config: pkgs: import ./src/devenv-devShell.nix { inherit config pkgs; };
      mkDocOptions = pkgs:
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
            { name = "${self}"; url = "https://github.com/cachix/devenv/blob/main"; }
            { name = "${git-hooks}"; url = "https://github.com/cachix/git-hooks.nix/blob/master"; }
          ];
          rewriteSource = decl:
            let
              prefix = lib.strings.concatStringsSep "/" (lib.lists.take 4 (lib.strings.splitString "/" decl));
              source = lib.lists.findFirst (src: src.name == prefix) { } sources;
              path = lib.strings.removePrefix prefix decl;
              url = "${source.url}${path}";
            in
            { name = url; url = url; };
          options = pkgs.nixosOptionsDoc {
            options = builtins.removeAttrs eval.options [ "_module" ];
            transformOptions = opt: (
              opt // { declarations = map rewriteSource opt.declarations; }
            );
          };
        in
        options;

    in
    {
      packages = forAllSystems (system:
        let
          inherit (pkgs) lib;
          pkgs = nixpkgs.legacyPackages.${system};
          options = mkDocOptions pkgs;
          filterOptions = import ./filterOptions.nix lib;
          evaluatedModules = pkgs.lib.evalModules {
            modules = [
              ./src/modules/top-level.nix
            ];
            specialArgs = { inherit pkgs inputs; };
          };
          generateKeyOptions = key:
            filterOptions
              (path: option:
                lib.any (lib.hasSuffix "/${key}.nix") option.declarations)
              evaluatedModules.options;

          optionsDocs = optionParameter: pkgs.nixosOptionsDoc {
            options = optionParameter;
            variablelistId = "options";
            transformOptions = options: removeAttrs options [ "declarations" ];
          };
        in
        {
          default = self.packages.${system}.devenv;
          devenv = mkPackage pkgs;
          devenv-docs-options = options.optionsCommonMark;
          devenv-docs-options-json = options.optionsJSON;
          devenv-generate-individual-docs =
            let
              inherit (pkgs) lib;
              languageOptions = builtins.mapAttrs (key: _: generateKeyOptions key) evaluatedModules.config.languages;
              serviceOptions = builtins.mapAttrs (key: _: generateKeyOptions key) evaluatedModules.config.services;
              processManagersOptions = builtins.mapAttrs (key: _: generateKeyOptions key) evaluatedModules.config.process.managers;
              processedOptions = option: builtins.mapAttrs (key: options: optionsDocs options) option;
            in
            pkgs.stdenv.mkDerivation {
              name = "generate-individual-docs";
              src = ./docs/individual-docs;
              buildPhase = ''
                languageDir=./languages
                serviceDir=./services
                processManagerDir=./process-managers
                mkdir -p $out/docs/individual-docs/supported-languages
                mkdir -p $out/docs/individual-docs/supported-services
                mkdir -p $out/docs/individual-docs/supported-process-managers
                AUTOGEN_NOTICE="[comment]: # (Do not edit this file as it is autogenerated. Go to docs/individual-docs if you want to make edits.)"

                ${lib.concatStringsSep "\n" (lib.mapAttrsToList (key: options:  ''
                  content=$(cat ${options.optionsCommonMark})
                  file=$languageDir/${key}.md

                  sed -i "1i$AUTOGEN_NOTICE" "$file"
                  substituteInPlace $file \
                  --subst-var-by \
                  AUTOGEN_OPTIONS \
                  "$content"

                  cp $file $out/docs/individual-docs/supported-languages/${key}.md

                '') ( processedOptions languageOptions ))}

                ${lib.concatStringsSep "\n" (lib.mapAttrsToList (key: options:  ''
                  content=$(cat ${options.optionsCommonMark})
                  file=$serviceDir/${key}.md

                  sed -i "1i$AUTOGEN_NOTICE" "$file"
                  substituteInPlace $file \
                  --subst-var-by \
                  AUTOGEN_OPTIONS \
                  "$content"

                  cp $file $out/docs/individual-docs/supported-services/${key}.md

                '') ( processedOptions serviceOptions ))}

                ${lib.concatStringsSep "\n" (lib.mapAttrsToList (key: options:  ''
                  content=$(cat ${options.optionsCommonMark})
                  file=$processManagerDir/${key}.md

                  sed -i "1i$AUTOGEN_NOTICE" "$file"
                  substituteInPlace $file \
                  --subst-var-by \
                  AUTOGEN_OPTIONS \
                  "$content"

                  cp $file $out/docs/individual-docs/supported-process-managers/${key}.md
                '') ( processedOptions  processManagersOptions))}
              '';
            };
        });

      modules = ./src/modules;
      isTmpDir = true;
      hasIsTesting = true;

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

      flakeModule = import ./flake-module.nix self;

      lib = {
        mkConfig = args@{ pkgs, inputs, modules }:
          (self.lib.mkEval args).config;
        mkEval = { pkgs, inputs, modules }:
          let
            moduleInputs = { inherit git-hooks; } // inputs;
            project = inputs.nixpkgs.lib.evalModules {
              specialArgs = moduleInputs // {
                inherit pkgs;
                inputs = moduleInputs;
              };
              modules = [
                (self.modules + /top-level.nix)
                ({ config, ... }: {
                  packages = pkgs.lib.mkBefore [
                    (mkDevShellPackage config pkgs)
                  ];
                  devenv.warnOnNewVersion = false;
                  devenv.flakesIntegration = true;
                })
              ] ++ modules;
            };
          in
          project;
        mkShell = args:
          let
            config = self.lib.mkConfig args;
          in
          config.shell // {
            ci = config.ciDerivation;
            inherit config;
          };
      };

      overlays.default = final: prev: {
        devenv = self.packages.${prev.system}.default;
      };
    };
}
