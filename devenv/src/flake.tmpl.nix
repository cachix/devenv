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
          getOverlays = inputName: inputAttrs:
            map
              (overlay:
                let
                  input = inputs.${inputName} or (throw "No such input `${inputName}` while trying to configure overlays.");
                in
                  input.overlays.${overlay} or (throw "Input `${inputName}` has no overlay called `${overlay}`. Supported overlays: ${nixpkgs.lib.concatStringsSep ", " (builtins.attrNames input.overlays)}"))
              inputAttrs.overlays or [ ];
          overlays = nixpkgs.lib.flatten (nixpkgs.lib.mapAttrsToList getOverlays (devenv.inputs or { }));
          permittedUnfreePackages = devenv.nixpkgs.per-platform."${system}".permittedUnfreePackages or devenv.nixpkgs.permittedUnfreePackages or [ ];
          pkgs = import nixpkgs {
            inherit overlays system;
            config = {
              allowUnfree = devenv.nixpkgs.per-platform."${system}".allowUnfree or devenv.nixpkgs.allowUnfree or devenv.allowUnfree or false;
              allowBroken = devenv.nixpkgs.per-platform."${system}".allowBroken or devenv.nixpkgs.allowBroken or devenv.allowBroken or false;
              cudaSupport = devenv.nixpkgs.per-platform."${system}".cudaSupport or devenv.nixpkgs.cudaSupport or false;
              cudaCapabilities = devenv.nixpkgs.per-platform."${system}".cudaCapabilities or devenv.nixpkgs.cudaCapabilities or [ ];
              permittedInsecurePackages = devenv.nixpkgs.per-platform."${system}".permittedInsecurePackages or devenv.nixpkgs.permittedInsecurePackages or devenv.permittedInsecurePackages or [ ];
              allowUnfreePredicate = if (permittedUnfreePackages != [ ]) then (pkg: builtins.elem (nixpkgs.lib.getName pkg) permittedUnfreePackages) else (_: false);
            };
          };
          lib = pkgs.lib;
          importModule = path:
            if lib.hasPrefix "./" path
            then if lib.hasSuffix ".nix" path
            then ./. + (builtins.substring 1 255 path)
            else ./. + (builtins.substring 1 255 path) + "/devenv.nix"
            else if lib.hasPrefix "../" path
            then throw "devenv: ../ is not supported for imports"
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
              {
                devenv.cliVersion = version;
                devenv.root = devenv_root;
                devenv.dotfile = devenv_dotfile;
              }
              ({ options, ... }: {
                config.devenv = lib.mkMerge [
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
              # Collect profiles to activate in priority order (lowest to highest precedence)
              manualProfiles = active_profiles;
              currentHostname = hostname;
              currentUsername = username;
              hostnameProfiles = lib.optional (currentHostname != "" && builtins.hasAttr currentHostname (baseProject.config.profiles.hostname or { })) "hostname.${currentHostname}";
              userProfiles = lib.optional (currentUsername != "" && builtins.hasAttr currentUsername (baseProject.config.profiles.user or { })) "user.${currentUsername}";

              # Priority groups: hostname (700) -> user (600) -> manual (500, 490, 480...)
              allProfileGroups = [
                { profiles = hostnameProfiles; basePriority = 99; }
                { profiles = userProfiles; basePriority = 94; }
                { profiles = manualProfiles; basePriority = 90; }
              ];

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

              # Process profile groups and apply priorities
              # lower number = higher priority
              processProfileGroup = { profiles, basePriority }:
                lib.flatten (lib.imap0 (groupIndex: profileName:
                  let
                    # Resolve all extended profiles for this profile
                    allProfileNames = resolveProfileExtends profileName [ ];
                  in
                  # Apply priorities to all resolved profiles
                  # Extended profiles get lower priority, current profile gets highest
                  lib.imap0 (profileIndex: resolvedProfileName:
                    let
                      # Calculate priority: base - (group * 100) - (profile * 10) - (extends * 1)
                      profilePriority = basePriority - profileIndex;
                      profileConfig = builtins.trace (resolvedProfileName) getProfileConfig resolvedProfileName;

                      applyModuleOverride = config:
                        if builtins.isFunction config
                        then (args:
                          let res = config args;
                          in builtins.trace
                              (builtins.toJSON res)
                              applyOverrideRecursive res
                        )
                        else builtins.trace (builtins.toJSON config) applyOverrideRecursive config;

                      # Apply priority overrides recursively to the deferredModule imports structure
                      # Need to apply override to actual config values, not container objects
                      applyOverrideRecursive = config:
                        if builtins.isFunction config
                        then config  # Don't modify functions - let module system handle them
                        else if lib.isAttrs config && config ? _type
                        then config  # Don't override values with existing type metadata
                        else if lib.isAttrs config
                        then lib.mapAttrs (_: applyOverrideRecursive) config
                        else lib.mkOverride profilePriority config;

                      prioritizedConfig = (
                        profileConfig.config // {
                          imports = lib.map (importItem:
                            importItem // {
                              imports = lib.map (nestedImport:
                                applyModuleOverride nestedImport
                              ) (importItem.imports or [ ]);
                            }
                          ) (profileConfig.config.imports or [ ]);
                        });
                    in
                    prioritizedConfig
                  ) allProfileNames
                ) profiles);

              # Collect all prioritized profile modules
              allPrioritizedModules = lib.flatten (map processProfileGroup allProfileGroups);
            in
            if allPrioritizedModules == [ ]
            then baseProject
            else 
              let
                finalProject = baseProject.extendModules { modules = allPrioritizedModules; };
              in
              # builtins.trace "=== FINAL MODULES === Count: ${toString (builtins.length allPrioritizedModules)} Environment: ${builtins.toJSON (finalProject.config.env or {})}" finalProject;
              finalProject;
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
                else
                  let v = build option config.${name};
                  in if v != { } then {
                    ${name} = v;
                  } else { }
              )
              options;

          systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
        in
        {
          devShell = lib.genAttrs systems (system: config.shell);
          packages = lib.genAttrs systems (system: {
            optionsJSON = options.optionsJSON;
            # deprecated
            inherit (config) info procfileScript procfileEnv procfile;
            ci = config.ciDerivation;
          });
          devenv = config;
          build = build project.options project.config;
        };
      }
