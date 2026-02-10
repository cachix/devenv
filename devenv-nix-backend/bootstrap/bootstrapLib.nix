# Shared library functions for devenv evaluation
{ inputs }:

rec {
  # Helper to get overlays for a given input
  getOverlays =
    inputName: inputAttrs:
    let
      lib = inputs.nixpkgs.lib;
    in
    map
      (
        overlay:
        let
          input =
            inputs.${inputName} or (throw "No such input `${inputName}` while trying to configure overlays.");
        in
          input.overlays.${overlay}
            or (throw "Input `${inputName}` has no overlay called `${overlay}`. Supported overlays: ${lib.concatStringsSep ", " (builtins.attrNames input.overlays)}")
      ) inputAttrs.overlays or [ ];

  # Main function to create devenv configuration for a specific system with profiles support
  # This is the full-featured version used by default.nix
  mkDevenvForSystem =
    { version
    , is_development_version ? false
    , system
    , devenv_root
    , git_root ? null
    , devenv_dotfile
    , devenv_dotfile_path
    , devenv_tmpdir
    , devenv_runtime
    , devenv_istesting ? false
    , devenv_sandbox ? null
    , devenv_direnvrc_latest_version
    , container_name ? null
    , active_profiles ? [ ]
    , hostname
    , username
    , cli_options ? [ ]
    , skip_local_src ? false
    , secretspec ? null
    , devenv_config ? { }
    , nixpkgs_config ? { }
    , lock_fingerprint ? null
    , primops ? { }
    }:
    let
      inherit (inputs) nixpkgs;
      lib = nixpkgs.lib;
      targetSystem = system;

      # devenv configuration is passed from the Rust backend
      overlays = lib.flatten (lib.mapAttrsToList getOverlays (devenv_config.inputs or { }));

      # Helper to create pkgs for a given system with nixpkgs_config
      mkPkgsForSystem =
        evalSystem:
        import nixpkgs {
          system = evalSystem;
          config = nixpkgs_config // {
            allowUnfreePredicate =
              if nixpkgs_config.allowUnfree or false then
                (_: true)
              else if (nixpkgs_config.permittedUnfreePackages or [ ]) != [ ] then
                (pkg: builtins.elem (lib.getName pkg) (nixpkgs_config.permittedUnfreePackages or [ ]))
              else
                (_: false);
          };
          inherit overlays;
        };

      pkgsBootstrap = mkPkgsForSystem targetSystem;

      # Helper to import a path, trying .nix first then /devenv.nix
      # Returns a list of modules, including devenv.local.nix when present
      tryImport =
        resolvedPath: basePath:
        if lib.hasSuffix ".nix" basePath then
          [ (import resolvedPath) ]
        else
          let
            devenvpath = resolvedPath + "/devenv.nix";
            localpath = resolvedPath + "/devenv.local.nix";
          in
          if builtins.pathExists devenvpath then
            [ (import devenvpath) ] ++ lib.optional (builtins.pathExists localpath) (import localpath)
          else
            throw (basePath + "/devenv.nix file does not exist");

      importModule =
        path:
        if lib.hasPrefix "path:" path then
        # path: prefix indicates a local filesystem path - strip it and import directly
          let
            actualPath = builtins.substring 5 999999 path;
          in
          tryImport (/. + actualPath) path
        else if lib.hasPrefix "/" path then
        # Absolute path - import directly (avoids input resolution and NAR hash computation)
          tryImport (/. + path) path
        else if lib.hasPrefix "./" path then
        # Relative paths are relative to devenv_root, not bootstrap directory
          let
            relPath = builtins.substring 1 255 path;
          in
          tryImport (/. + devenv_root + relPath) path
        else if lib.hasPrefix "../" path then
        # Parent relative paths also relative to devenv_root
          tryImport (/. + devenv_root + "/${path}") path
        else
          let
            paths = lib.splitString "/" path;
            name = builtins.head paths;
            input = inputs.${name} or (throw "Unknown input ${name}");
            subpath = "/${lib.concatStringsSep "/" (builtins.tail paths)}";
            devenvpath = input + subpath;
          in
          tryImport devenvpath path;

      # Common modules shared between main evaluation and cross-system evaluation
      mkCommonModules =
        evalPkgs:
        [
          (
            { config, ... }:
            {
              _module.args.pkgs = evalPkgs.appendOverlays (config.overlays or [ ]);
              _module.args.secretspec = secretspec;
              _module.args.devenvPrimops = primops;
              _module.args.devenvSandbox = devenv_sandbox;
            }
          )
          (inputs.devenv.modules + /top-level.nix)
          (
            { options, ... }:
            {
              config.devenv = lib.mkMerge [
                {
                  root = devenv_root;
                  dotfile = devenv_dotfile;
                }
                (
                  if builtins.hasAttr "cli" options.devenv then
                    {
                      cli.version = version;
                      cli.isDevelopment = is_development_version;
                    }
                  else
                    {
                      cliVersion = version;
                    }
                )
                (lib.optionalAttrs (builtins.hasAttr "tmpdir" options.devenv) {
                  tmpdir = devenv_tmpdir;
                })
                (lib.optionalAttrs (builtins.hasAttr "isTesting" options.devenv) {
                  isTesting = devenv_istesting;
                })
                (lib.optionalAttrs (builtins.hasAttr "runtime" options.devenv) {
                  runtime = devenv_runtime;
                })
                (lib.optionalAttrs (builtins.hasAttr "direnvrcLatestVersion" options.devenv) {
                  direnvrcLatestVersion = devenv_direnvrc_latest_version;
                })
              ];
            }
          )
          (
            { options, ... }:
            {
              config = lib.mkMerge [
                (lib.optionalAttrs (builtins.hasAttr "git" options) {
                  git.root = git_root;
                })
              ];
            }
          )
          (lib.optionalAttrs (container_name != null) {
            container.isBuilding = lib.mkForce true;
            containers.${container_name}.isBuilding = true;
          })
        ]
        ++ (lib.flatten (map importModule (devenv_config.imports or [ ])))
        ++ (if !skip_local_src then (importModule (devenv_root + "/devenv.nix")) else [ ])
        ++ [
          (devenv_config.devenv or { })
          (
            let
              localPath = devenv_root + "/devenv.local.nix";
            in
            if builtins.pathExists localPath then import localPath else { }
          )
          cli_options
        ];

      # Phase 1: Base evaluation to extract profile definitions
      baseProject = lib.evalModules {
        specialArgs = inputs // {
          inherit inputs secretspec primops;
        };
        modules = mkCommonModules pkgsBootstrap;
      };

      # Phase 2: Extract and apply profiles using extendModules with priority overrides
      project =
        let
          # Build ordered list of profile names: hostname -> user -> manual
          manualProfiles = active_profiles;
          currentHostname = hostname;
          currentUsername = username;
          hostnameProfiles = lib.optional
            (
              currentHostname != null
              && currentHostname != ""
              && builtins.hasAttr currentHostname (baseProject.config.profiles.hostname or { })
            ) "hostname.${currentHostname}";
          userProfiles = lib.optional
            (
              currentUsername != null
              && currentUsername != ""
              && builtins.hasAttr currentUsername (baseProject.config.profiles.user or { })
            ) "user.${currentUsername}";

          # Ordered list of profiles to activate
          orderedProfiles = hostnameProfiles ++ userProfiles ++ manualProfiles;

          # Resolve profile extends with cycle detection
          resolveProfileExtends =
            profileName: visited:
            if builtins.elem profileName visited then
              throw "Circular dependency detected in profile extends: ${lib.concatStringsSep " -> " visited} -> ${profileName}"
            else
              let
                profile = getProfileConfig profileName;
                extends = profile.extends or [ ];
                newVisited = visited ++ [ profileName ];
                extendedProfiles = lib.flatten (map (name: resolveProfileExtends name newVisited) extends);
              in
              extendedProfiles ++ [ profileName ];

          # Get profile configuration by name from baseProject
          getProfileConfig =
            profileName:
            if lib.hasPrefix "hostname." profileName then
              let
                name = lib.removePrefix "hostname." profileName;
              in
              baseProject.config.profiles.hostname.${name}
            else if lib.hasPrefix "user." profileName then
              let
                name = lib.removePrefix "user." profileName;
              in
              baseProject.config.profiles.user.${name}
            else
              let
                availableProfiles = builtins.attrNames (baseProject.config.profiles or { });
                hostnameProfiles = map (n: "hostname.${n}") (
                  builtins.attrNames (baseProject.config.profiles.hostname or { })
                );
                userProfiles = map (n: "user.${n}") (builtins.attrNames (baseProject.config.profiles.user or { }));
                allAvailableProfiles = availableProfiles ++ hostnameProfiles ++ userProfiles;
              in
                baseProject.config.profiles.${profileName}
                  or (throw "Profile '${profileName}' not found. Available profiles: ${lib.concatStringsSep ", " allAvailableProfiles}");

          # Fold over ordered profiles to build final list with extends
          expandedProfiles = lib.foldl'
            (
              acc: profileName:
                let
                  allProfileNames = resolveProfileExtends profileName [ ];
                in
                acc ++ allProfileNames
            ) [ ]
            orderedProfiles;

          # Map over expanded profiles and apply priorities
          allPrioritizedModules = lib.imap0
            (
              index: profileName:
                let
                  profilePriority = (lib.modules.defaultOverridePriority - 1) - index;
                  profileConfig = getProfileConfig profileName;

                  typeNeedsOverride =
                    type:
                    if type == null then
                      false
                    else
                      let
                        typeName = type.name or type._type or "";

                        isLeafType = builtins.elem typeName [
                          "str"
                          "int"
                          "bool"
                          "enum"
                          "path"
                          "package"
                          "float"
                          "anything"
                        ];
                      in
                      if isLeafType then
                        true
                      else if typeName == "nullOr" then
                        let
                          innerType =
                            type.elemType
                              or (if type ? nestedTypes && type.nestedTypes ? elemType then type.nestedTypes.elemType else null);
                        in
                        if innerType != null then typeNeedsOverride innerType else false
                      else
                        false;

                  pathNeedsOverride =
                    optionPath:
                    let
                      directOption = lib.attrByPath optionPath null baseProject.options;
                    in
                    if directOption != null && lib.isOption directOption then
                      typeNeedsOverride directOption.type
                    else if optionPath != [ ] then
                      let
                        parentPath = lib.init optionPath;
                        parentOption = lib.attrByPath parentPath null baseProject.options;
                      in
                      if parentOption != null && lib.isOption parentOption then
                        let
                          freeformType = parentOption.type.freeformType or parentOption.type.nestedTypes.freeformType or null;
                          elementType =
                            if freeformType ? elemType then
                              freeformType.elemType
                            else if freeformType ? nestedTypes && freeformType.nestedTypes ? elemType then
                              freeformType.nestedTypes.elemType
                            else
                              freeformType;
                        in
                        typeNeedsOverride elementType
                      else
                        false
                    else
                      false;

                  applyModuleOverride =
                    config:
                    if builtins.isFunction config then
                      let
                        wrapper = args: applyOverrideRecursive (config args) [ ];
                      in
                      lib.mirrorFunctionArgs config wrapper
                    else
                      applyOverrideRecursive config [ ];

                  applyOverrideRecursive =
                    config: optionPath:
                    if lib.isAttrs config && config ? _type then
                      config
                    else if lib.isAttrs config then
                      lib.mapAttrs (name: value: applyOverrideRecursive value (optionPath ++ [ name ])) config
                    else if pathNeedsOverride optionPath then
                      lib.mkOverride profilePriority config
                    else
                      config;

                  prioritizedConfig = (
                    profileConfig.module
                    // {
                      imports = lib.map
                        (
                          importItem:
                          importItem
                          // {
                            imports = lib.map (nestedImport: applyModuleOverride nestedImport) (importItem.imports or [ ]);
                          }
                        )
                        (profileConfig.module.imports or [ ]);
                    }
                  );
                in
                prioritizedConfig
            )
            expandedProfiles;
        in
        if allPrioritizedModules == [ ] then
          baseProject
        else
          baseProject.extendModules { modules = allPrioritizedModules; };

      config = project.config;

      # Apply config overlays to pkgs
      pkgs = pkgsBootstrap.appendOverlays (config.overlays or [ ]);

      options = pkgs.nixosOptionsDoc {
        options = builtins.removeAttrs project.options [ "_module" ];
        warningsAreErrors = false;
        transformOptions =
          let
            isDocType =
              v:
              builtins.elem v [
                "literalDocBook"
                "literalExpression"
                "literalMD"
                "mdDoc"
              ];
          in
          lib.attrsets.mapAttrs (
            _: v:
              if v ? _type && isDocType v._type then
                v.text
              else if v ? _type && v._type == "derivation" then
                v.name
              else
                v
          );
      };

      build =
        options: config:
        lib.concatMapAttrs
          (
            name: option:
            if lib.isOption option then
              let
                typeName = option.type.name or "";
              in
              if
                builtins.elem typeName [
                  "output"
                  "outputOf"
                ]
              then
                {
                  ${name} = config.${name};
                }
              else
                { }
            else if builtins.isAttrs option && !lib.isDerivation option then
              let
                v = build option config.${name};
              in
              if v != { } then { ${name} = v; } else { }
            else
              { }
          )
          options;

      # Helper to evaluate devenv for a specific system (for cross-compilation, e.g. macOS building Linux containers)
      evalForSystem =
        evalSystem:
        let
          evalPkgs = mkPkgsForSystem evalSystem;
          evalProject = lib.evalModules {
            specialArgs = inputs // {
              inherit inputs secretspec primops;
            };
            modules = mkCommonModules evalPkgs;
          };
        in
        {
          config = evalProject.config;
        };

      # All supported systems for cross-compilation (lazily evaluated)
      allSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      # Generate perSystem entries for all systems (only evaluated when accessed)
      perSystemConfigs = lib.genAttrs allSystems (
        perSystem: if perSystem == targetSystem then { config = config; } else evalForSystem perSystem
      );
    in
    {
      inherit
        pkgs
        config
        options
        project
        ;
      bash = pkgs.bash;
      shell = config.shell;
      optionsJSON = options.optionsJSON;
      info = config.info;
      ci = config.ciDerivation;
      build = build project.options config;
      devenv = {
        # Backwards compatibility: wrap config in devenv attribute for code expecting devenv.config.*
        config = config;
        # perSystem structure for cross-compilation (e.g. macOS building Linux containers)
        perSystem = perSystemConfigs;
      };
    };

  # Simplified devenv evaluation for inputs
  # This is a lightweight version suitable for evaluating an input's devenv.nix
  mkDevenvForInput =
    {
      # The input to evaluate (must have outPath and sourceInfo)
      input
    , # All resolved inputs (for specialArgs)
      allInputs
    , # System to evaluate for
      system ? builtins.currentSystem
    , # Nixpkgs to use (defaults to allInputs.nixpkgs)
      nixpkgs ? allInputs.nixpkgs or (throw "nixpkgs input required")
    , # Devenv modules (defaults to allInputs.devenv)
      devenv ? allInputs.devenv or (throw "devenv input required")
    ,
    }:
    let
      devenvPath = input.outPath + "/devenv.nix";
      hasDevenv = builtins.pathExists devenvPath;
    in
    if !hasDevenv then
      throw ''
        Input does not have a devenv.nix file.
        Expected file at: ${devenvPath}

        To use this input's devenv configuration, the input must provide a devenv.nix file.
      ''
    else
      let
        pkgs = import nixpkgs {
          inherit system;
          config = { };
        };
        lib = pkgs.lib;

        project = lib.evalModules {
          specialArgs = allInputs // {
            inputs = allInputs;
            secretspec = null;
          };
          modules = [
            (
              { config, ... }:
              {
                _module.args.pkgs = pkgs.appendOverlays (config.overlays or [ ]);
              }
            )
            (devenv.outPath + "/src/modules/top-level.nix")
            (import devenvPath)
          ];
        };
      in
      {
        inherit pkgs;
        config = project.config;
        options = project.options;
        inherit project;
      };
}
