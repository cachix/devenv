{
  inputs =
    let
      __DEVENV_VARS__
        in {
        git-hooks.url = "github:cachix/git-hooks.nix";
      git-hooks.inputs.nixpkgs.follows = "nixpkgs";
      pre-commit-hooks.follows = "git-hooks";
      nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
      devenv.url = "github:cachix/devenv?dir=src/modules";
      } // (if builtins.pathExists (devenv_dotfile_path + "/flake.json")
      then builtins.fromJSON (builtins.readFile (devenv_dotfile_path +  "/flake.json"))
      else { });

      outputs = { nixpkgs, ... }@inputs:
        let
          __DEVENV_VARS__
            devenv =
            if builtins.pathExists (devenv_dotfile_path + "/devenv.json")
            then builtins.fromJSON (builtins.readFile (devenv_dotfile_path + "/devenv.json"))
            else { };

          systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

          # Function to create devenv configuration for a specific system with profiles support
          mkDevenvForSystem = targetSystem:
            let
              getOverlays = inputName: inputAttrs:
                map
                  (overlay:
                    let
                      input = inputs.${inputName} or (throw "No such input `${inputName}` while trying to configure overlays.");
                    in
                      input.overlays.${overlay} or (throw "Input `${inputName}` has no overlay called `${overlay}`. Supported overlays: ${nixpkgs.lib.concatStringsSep ", " (builtins.attrNames input.overlays)}"))
                  inputAttrs.overlays or [ ];
              overlays = nixpkgs.lib.flatten (nixpkgs.lib.mapAttrsToList getOverlays (devenv.inputs or { }));
              permittedUnfreePackages = devenv.nixpkgs.per-platform."${targetSystem}".permittedUnfreePackages or devenv.nixpkgs.permittedUnfreePackages or [ ];
              pkgs = import nixpkgs {
                system = targetSystem;
                config = {
                  allowUnfree = devenv.nixpkgs.per-platform."${targetSystem}".allowUnfree or devenv.nixpkgs.allowUnfree or devenv.allowUnfree or false;
                  allowBroken = devenv.nixpkgs.per-platform."${targetSystem}".allowBroken or devenv.nixpkgs.allowBroken or devenv.allowBroken or false;
                  cudaSupport = devenv.nixpkgs.per-platform."${targetSystem}".cudaSupport or devenv.nixpkgs.cudaSupport or false;
                  cudaCapabilities = devenv.nixpkgs.per-platform."${targetSystem}".cudaCapabilities or devenv.nixpkgs.cudaCapabilities or [ ];
                  permittedInsecurePackages = devenv.nixpkgs.per-platform."${targetSystem}".permittedInsecurePackages or devenv.nixpkgs.permittedInsecurePackages or devenv.permittedInsecurePackages or [ ];
                  allowUnfreePredicate = if (permittedUnfreePackages != [ ]) then (pkg: builtins.elem (nixpkgs.lib.getName pkg) permittedUnfreePackages) else (_: false);
                };
                inherit overlays;
              };
              lib = pkgs.lib;
              importModule = path:
                if lib.hasPrefix "./" path
                then if lib.hasSuffix ".nix" path
                then ./. + (builtins.substring 1 255 path)
                else ./. + (builtins.substring 1 255 path) + "/devenv.nix"
                else if lib.hasPrefix "../" path
                then
                # For parent directory paths, concatenate with /.
                # ./. refers to the directory containing this file (project root)
                # So ./. + "/../shared" = <project-root>/../shared
                  if lib.hasSuffix ".nix" path
                  then ./. + "/${path}"
                  else ./. + "/${path}/devenv.nix"
                else
                  let
                    paths = lib.splitString "/" path;
                    name = builtins.head paths;
                    input = inputs.${name} or (throw "Unknown input ${name}");
                    subpath = "/${lib.concatStringsSep "/" (builtins.tail paths)}";
                    devenvpath = "${input}" + subpath;
                    devenvdefaultpath = devenvpath + "/devenv.nix";
                  in
                  if lib.hasSuffix ".nix" devenvpath
                  then devenvpath
                  else if builtins.pathExists devenvdefaultpath
                  then devenvdefaultpath
                  else throw (devenvdefaultpath + " file does not exist for input ${name}.");

              # Phase 1: Base evaluation to extract profile definitions
              baseProject = pkgs.lib.evalModules {
                specialArgs = inputs // { inherit inputs; };
                modules = [
                  ({ config, ... }: {
                    _module.args.pkgs = pkgs.appendOverlays (config.overlays or [ ]);
                  })
                  (inputs.devenv.modules + /top-level.nix)
                  ({ options, ... }: {
                    config.devenv = lib.mkMerge [
                      {
                        cliVersion = version;
                        root = devenv_root;
                        dotfile = devenv_dotfile;
                      }
                      (pkgs.lib.optionalAttrs (builtins.hasAttr "tmpdir" options.devenv) {
                        tmpdir = devenv_tmpdir;
                      })
                      (pkgs.lib.optionalAttrs (builtins.hasAttr "isTesting" options.devenv) {
                        isTesting = devenv_istesting;
                      })
                      (pkgs.lib.optionalAttrs (builtins.hasAttr "runtime" options.devenv) {
                        runtime = devenv_runtime;
                      })
                      (pkgs.lib.optionalAttrs (builtins.hasAttr "direnvrcLatestVersion" options.devenv) {
                        direnvrcLatestVersion = devenv_direnvrc_latest_version;
                      })
                    ];
                  })
                  ({ options, ... }: {
                    config = lib.mkMerge [
                      (pkgs.lib.optionalAttrs (builtins.hasAttr "git" options) {
                        git.root = git_root;
                      })
                    ];
                  })
                  (pkgs.lib.optionalAttrs (container_name != null) {
                    container.isBuilding = pkgs.lib.mkForce true;
                    containers.${container_name}.isBuilding = true;
                  })
                ] ++ (map importModule (devenv.imports or [ ])) ++ [
                  (if builtins.pathExists ./devenv.nix then ./devenv.nix else { })
                  (devenv.devenv or { })
                  (if builtins.pathExists ./devenv.local.nix then ./devenv.local.nix else { })
                  (if builtins.pathExists (devenv_dotfile_path + "/cli-options.nix") then import (devenv_dotfile_path + "/cli-options.nix") else { })
                ];
              };

              # Phase 2: Extract and apply profiles using extendModules with priority overrides
              project =
                let
                  # Build ordered list of profile names: hostname -> user -> manual
                  manualProfiles = active_profiles;
                  currentHostname = hostname;
                  currentUsername = username;
                  hostnameProfiles = lib.optional (currentHostname != "" && builtins.hasAttr currentHostname (baseProject.config.profiles.hostname or { })) "hostname.${currentHostname}";
                  userProfiles = lib.optional (currentUsername != "" && builtins.hasAttr currentUsername (baseProject.config.profiles.user or { })) "user.${currentUsername}";

                  # Ordered list of profiles to activate
                  orderedProfiles = hostnameProfiles ++ userProfiles ++ manualProfiles;

                  # Resolve profile extends with cycle detection
                  resolveProfileExtends = profileName: visited:
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
                  getProfileConfig = profileName:
                    if lib.hasPrefix "hostname." profileName then
                      let name = lib.removePrefix "hostname." profileName;
                      in baseProject.config.profiles.hostname.${name}
                    else if lib.hasPrefix "user." profileName then
                      let name = lib.removePrefix "user." profileName;
                      in baseProject.config.profiles.user.${name}
                    else
                      let
                        availableProfiles = builtins.attrNames (baseProject.config.profiles or { });
                        hostnameProfiles = map (n: "hostname.${n}") (builtins.attrNames (baseProject.config.profiles.hostname or { }));
                        userProfiles = map (n: "user.${n}") (builtins.attrNames (baseProject.config.profiles.user or { }));
                        allAvailableProfiles = availableProfiles ++ hostnameProfiles ++ userProfiles;
                      in
                        baseProject.config.profiles.${profileName} or (throw "Profile '${profileName}' not found. Available profiles: ${lib.concatStringsSep ", " allAvailableProfiles}");

                  # Fold over ordered profiles to build final list with extends
                  expandedProfiles = lib.foldl'
                    (acc: profileName:
                      let
                        allProfileNames = resolveProfileExtends profileName [ ];
                      in
                      acc ++ allProfileNames
                    ) [ ]
                    orderedProfiles;

                  # Map over expanded profiles and apply priorities
                  allPrioritizedModules = lib.imap0
                    (index: profileName:
                      let
                        # Decrement priority for each profile (lower = higher precedence)
                        # Start with the next lowest priority after the default priority for values (100)
                        profilePriority = (lib.modules.defaultOverridePriority - 1) - index;
                        profileConfig = getProfileConfig profileName;

                        # Check if an option type needs explicit override to resolve conflicts
                        # Only apply overrides to LEAF values (scalars), not collection types that can merge
                        typeNeedsOverride = type:
                          if type == null then false
                          else
                            let
                              typeName = type.name or type._type or "";

                              # True leaf types that need priority resolution when they conflict
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
                            if isLeafType then true
                            else if typeName == "nullOr" then
                            # For nullOr, check the wrapped type recursively
                              let
                                innerType = type.elemType or
                                  (if type ? nestedTypes && type.nestedTypes ? elemType
                                  then type.nestedTypes.elemType
                                  else null);
                              in
                              if innerType != null then typeNeedsOverride innerType else false
                            else
                            # Everything else (collections, submodules, etc.) should merge naturally
                              false;

                        # Check if a config path needs explicit override
                        pathNeedsOverride = optionPath:
                          let
                            # Try direct option first
                            directOption = lib.attrByPath optionPath null baseProject.options;
                          in
                          if directOption != null && lib.isOption directOption then
                            typeNeedsOverride directOption.type
                          else if optionPath != [ ] then
                          # Check parent for freeform type
                            let
                              parentPath = lib.init optionPath;
                              parentOption = lib.attrByPath parentPath null baseProject.options;
                            in
                            if parentOption != null && lib.isOption parentOption then
                              let
                                # Look for freeform type:
                                # 1. Standard location: type.freeformType (primary)
                                # 2. Nested location: type.nestedTypes.freeformType (evaluated form)
                                freeformType = parentOption.type.freeformType or
                                  parentOption.type.nestedTypes.freeformType or
                                    null;
                                elementType =
                                  if freeformType ? elemType then freeformType.elemType
                                  else if freeformType ? nestedTypes && freeformType.nestedTypes ? elemType then freeformType.nestedTypes.elemType
                                  else freeformType;
                              in
                              typeNeedsOverride elementType
                            else false
                          else false;

                        # Support overriding both plain attrset modules and functions
                        applyModuleOverride = config:
                          if builtins.isFunction config
                          then
                            let
                              wrapper = args: applyOverrideRecursive (config args) [ ];
                            in
                            lib.mirrorFunctionArgs config wrapper
                          else applyOverrideRecursive config [ ];

                        # Apply overrides recursively based on option types
                        applyOverrideRecursive = config: optionPath:
                          if lib.isAttrs config && config ? _type then
                            config  # Don't touch values with existing type metadata
                          else if lib.isAttrs config then
                            lib.mapAttrs (name: value: applyOverrideRecursive value (optionPath ++ [ name ])) config
                          else if pathNeedsOverride optionPath then
                            lib.mkOverride profilePriority config
                          else
                            config;

                        # Apply priority overrides recursively to the deferredModule imports structure
                        prioritizedConfig = (
                          profileConfig.module // {
                            imports = lib.map
                              (importItem:
                                importItem // {
                                  imports = lib.map
                                    (nestedImport:
                                      applyModuleOverride nestedImport
                                    )
                                    (importItem.imports or [ ]);
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
                if allPrioritizedModules == [ ]
                then baseProject
                else baseProject.extendModules { modules = allPrioritizedModules; };

              config = project.config;

              options = pkgs.nixosOptionsDoc {
                options = builtins.removeAttrs project.options [ "_module" ];
                warningsAreErrors = false;
                # Unpack Nix types, e.g. literalExpression, mDoc.
                transformOptions =
                  let isDocType = v: builtins.elem v [ "literalDocBook" "literalExpression" "literalMD" "mdDoc" ];
                  in lib.attrsets.mapAttrs (_: v:
                    if v ? _type && isDocType v._type then
                      v.text
                    else if v ? _type && v._type == "derivation" then
                      v.name
                    else
                      v
                  );
              };

              # Recursively search for outputs in the config.
              # This is used when not building a specific output by attrpath.
              build = options: config:
                lib.concatMapAttrs
                  (name: option:
                    if lib.isOption option then
                      let typeName = option.type.name or "";
                      in
                      if builtins.elem typeName [ "output" "outputOf" ] then
                        { ${name} = config.${name}; }
                      else { }
                    else if builtins.isAttrs option && !lib.isDerivation option then
                      let v = build option config.${name};
                      in if v != { } then {
                        ${name} = v;
                      } else { }
                    else { }
                  )
                  options;
            in
            {
              inherit config options build project;
              shell = config.shell;
              packages = {
                optionsJSON = options.optionsJSON;
                # deprecated
                inherit (config) info procfileScript procfileEnv procfile;
                ci = config.ciDerivation;
              };
            };

          # Generate per-system devenv configurations
          perSystem = nixpkgs.lib.genAttrs systems mkDevenvForSystem;

          # Default devenv for the current system
          currentSystemDevenv = perSystem.${system};
        in
        {
          devShell = nixpkgs.lib.genAttrs systems (s: perSystem.${s}.shell);
          packages = nixpkgs.lib.genAttrs systems (s: perSystem.${s}.packages);

          # Per-system devenv configurations
          devenv = {
            # Default devenv for the current system
            inherit (currentSystemDevenv) config options build shell packages project;
            # Per-system devenv configurations
            inherit perSystem;
          };

          # Legacy build output
          build = currentSystemDevenv.build currentSystemDevenv.options currentSystemDevenv.config;
        };
      }
