devenvFlake: { flake-parts-lib, lib, inputs, ... }: {
  options.perSystem = flake-parts-lib.mkPerSystemOption ({ config, pkgs, system, ... }:

    let
      devenvType = (devenvFlake.lib.mkEval {
        inherit inputs lib pkgs;
        # Add flake-parts-specific config here if necessary
        inherit (config.devenv) modules;
      }).type;

      shellPrefix = shellName: if shellName == "default" then "" else "${shellName}-";
    in

    {
      options.devenv.modules = lib.mkOption {
        type = lib.types.listOf lib.types.deferredModule;
        description = ''
          Extra modules to import into every shell.
          Allows flakeModules to add options to devenv for example.
        '';
        default = [
          devenvFlake.flakeModules.readDevenvRoot
        ];
      };
      options.devenv.shells = lib.mkOption {
        type = lib.types.lazyAttrsOf devenvType;
        description = ''
          The [devenv.sh](https://devenv.sh) settings, per shell.

          Each definition `devenv.shells.<name>` results in a value for
          [`devShells.<name>`](flake-parts.html#opt-perSystem.devShells).

          Define `devenv.shells.default` for the default `nix develop`
          invocation - without an argument.
        '';
        example = lib.literalExpression ''
          {
            # create devShells.default
            default = {
              # devenv settings, e.g.
              languages.elm.enable = true;
            };
          }
        '';
        default = { };
      };
      options.devenv.git-hooks = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = (inputs.git-hooks or null) != null;
          description = "Enable git-hooks git-hooks.";
        };
        shell = lib.mkOption {
          type = lib.types.str;
          default = "default";
          description = "Shell name to read git-hooks git-hooks configuration from.";
        };
      };
      options.devenv.treefmt = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = (inputs.treefmt-nix or null) != null;
          description = "Enable treefmt.";
        };
        shell = lib.mkOption {
          type = lib.types.str;
          default = "default";
          description = "Shell name to read treefmt configuration from.";
        };
      };

      config.devShells = lib.mapAttrs (_name: devenv: devenv.shell) config.devenv.shells;

      config.pre-commit.settings =
          if config.devenv.git-hooks.enable && builtins.hasAttr config.devenv.git-hooks.shell config.devenv.shells then
            config.devenv.shells.${config.devenv.git-hooks.shell}.git-hooks
          else
            { };

      config.treefmt =
          if config.devenv.treefmt.enable && builtins.hasAttr config.devenv.treefmt.shell config.devenv.shells then
            config.devenv.shells.${config.devenv.treefmt.shell}.treefmt.config
          else
            { };

      # Deprecated packages
      # These were used to wire up commands in the devenv shim and are no longer necessary.
      config.packages =
        let
          deprecate = name: value: lib.warn "The package '${name}' is deprecated. Use the corresponding `devenv <cmd>` commands." value;
        in
        lib.optionalAttrs (lib.hasSuffix "-linux" system)
          (
            lib.concatMapAttrs
              (shellName: devenv:
                # TODO(sander): container support is undocumented and is specific to flake-parts, ie. the CLI shim doesn't support this.
                # Official support is complicated by `getInput` throwing errors and Nix not being able to properly try/catch errors with `tryEval`.
                # Until this is fixed, these outputs will remain.
                lib.concatMapAttrs
                  (containerName: container:
                    { "${shellPrefix shellName}container-${containerName}" = container.derivation; }
                  )
                  devenv.containers
              )
              config.devenv.shells
          ) // lib.concatMapAttrs
          (shellName: devenv:
            lib.mapAttrs deprecate {
              "${shellPrefix shellName}devenv-up" = devenv.procfileScript;
              "${shellPrefix shellName}devenv-test" = devenv.test;
            }
          )
          config.devenv.shells;
    });

  # the extra parameter before the module make this module behave like an
  # anonymous module, so we need to manually identify the file, for better
  # error messages, docs, and deduplication.
  _file = __curPos.file;
}
