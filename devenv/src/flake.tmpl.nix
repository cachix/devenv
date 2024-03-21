{
  inputs =
    let
      __DEVENV_VARS__
        in {
        pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
      pre-commit-hooks.inputs.nixpkgs.follows = "nixpkgs";
      nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
      devenv.url = "github:cachix/devenv?dir=src/modules";
      } // (if builtins.pathExists (devenv_dotfile + "/flake.json")
      then builtins.fromJSON (builtins.readFile (devenv_dotfile +  "/flake.json"))
      else { });

      outputs = { nixpkgs, ... }@inputs:
        let
          __DEVENV_VARS__
            devenv =
            if builtins.pathExists (devenv_dotfile + "/devenv.json")
            then builtins.fromJSON (builtins.readFile (devenv_dotfile + "/devenv.json"))
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
          pkgs = import nixpkgs {
            inherit system;
            config = {
              allowUnfree = devenv.allowUnfree or false;
              allowBroken = devenv.allowBroken or false;
              permittedInsecurePackages = devenv.permittedInsecurePackages or [ ];
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
            then throw "devenv: ../ is not supported for imports"
            else
              let
                paths = lib.splitString "/" path;
                name = builtins.head paths;
                input = inputs.${name} or (throw "Unknown input ${name}");
                subpath = "/${lib.concatStringsSep "/" (builtins.tail paths)}";
                devenvpath = "${input}" + subpath + "/devenv.nix";
              in
              if builtins.pathExists devenvpath
              then devenvpath
              else throw (devenvpath + " file does not exist for input ${name}.");
          project = pkgs.lib.evalModules {
            specialArgs = inputs // { inherit inputs pkgs; };
            modules = [
              (inputs.devenv.modules + /top-level.nix)
              {
                devenv.cliVersion = version;
                devenv.root = devenv_root;
                devenv.dotfile = pkgs.lib.mkForce (devenv_root + "/" + devenv_dotfile_string);
              }
              (pkgs.lib.optionalAttrs (inputs.devenv.isTmpDir or false) {
                devenv.tmpdir = tmpdir;
              })
              (pkgs.lib.optionalAttrs (container_name != null) {
                container.isBuilding = pkgs.lib.mkForce true;
                containers.${container_name}.isBuilding = true;
              })
            ] ++ (map importModule (devenv.imports or [ ])) ++ [
              ./devenv.nix
              (devenv.devenv or { })
              (if builtins.pathExists ./devenv.local.nix then ./devenv.local.nix else { })
            ];
          };
          config = project.config;

          options = pkgs.nixosOptionsDoc {
            options = builtins.removeAttrs project.options [ "_module" ];
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
        in
        {
          packages."${system}" = {
            optionsJSON = options.optionsJSON;
            inherit (config) info procfileScript procfileEnv procfile;
            ci = config.ciDerivation;
          };
          devenv = config;
          devShell."${system}" = config.shell;
        };
      }
