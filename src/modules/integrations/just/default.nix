# This largely inspired by the use of freeformType in
# https://github.com/cachix/git-hooks.nix/blob/master/modules/hooks.nix
# as well as https://github.com/juspay/just-flake/tree/main
{ pkgs, lib, config, ... }:

let
  inherit (lib) types;
  featureMod = {
    imports = [ ./feature.nix ];
    config._module.args = { inherit pkgs; };
  };
  featureType = types.submodule featureMod;

  staticFeatures = {
    convco = lib.mkOption {
      description = "Add the 'changelog' target calling convco";
      type = types.submodule { imports = [ featureMod ]; };
    };
    rust = lib.mkOption {
      description = "Add 'w' and 'test' targets for running cargo";
      type = types.submodule { imports = [ featureMod ]; };
    };
    treefmt = lib.mkOption {
      description = "Add the 'fmt' target to format source tree using treefmt";
      type = types.submodule { imports = [ featureMod ]; };
    };
    up = lib.mkOption {
      description = "Starts processes in foreground. See http://devenv.sh/processes";
      type = types.submodule { imports = [ featureMod ]; };
    };
    version = lib.mkOption {
      description = "Display devenv version";
      type = types.submodule { imports = [ featureMod ]; };
    };
  };

  scriptFeatures = lib.genAttrs (builtins.attrNames config.scripts) (name:
    let
      script = config.scripts.${name};
    in
    lib.mkOption {
      description = script.description;
      type = types.submodule {
        imports = [ featureMod ];
      };
    });

in
{

  imports = [{
    options.just.features = lib.mkOption {
      type = types.submoduleWith {
        modules = [{ freeformType = types.attrsOf featureType; }];
        specialArgs = { inherit pkgs; };
      };
      default = { };
    };
  }];

  options.just = {
    enable = lib.mkEnableOption "the just command runner";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.just;
      defaultText = lib.literalExpression "pkgs.just";
      description = "The just package to use.";
    };

    commonFileName = lib.mkOption {
      type = lib.types.str;
      default = "just-flake.just";
      description = ''
        The name of the common justfile generated by this module.
      '';
    };

    features = lib.recursiveUpdate staticFeatures scriptFeatures;
  };

  config = {

    packages = [
      config.just.package
    ] ++ lib.optionals config.just.features.convco.enable ([
      pkgs.convco
    ]);

    # NOTE: At somepoint, we may want to add `settings` options to some of these features.
    just.features = lib.recursiveUpdate
      (lib.mapAttrs (_: lib.mapAttrs (_: lib.mkDefault)) {
        convco = {
          enable = config.pre-commit.hooks.convco.enable;
          justfile = ''
            # Generate CHANGELOG.md using recent commits
            changelog:
              convco changelog -p "" > CHANGELOG.md
          '';
        };
        rust = {
          justfile = ''
            # Compile and watch the project
            w:
              cargo watch

            # Run and watch 'cargo test'
            test:
              cargo watch -s "cargo test"
          '';
        };
        treefmt = {
          enable = config.pre-commit.hooks.treefmt.enable;
          justfile = ''
            # Auto-format the source tree using treefmt
            fmt:
              treefmt
          '';
        };
        up = {
          enable = true;
          justfile = ''
            # Starts processes in foreground. See http://devenv.sh/processes
            up:
              devenv up
          '';
        };
        version = {
          enable = true;
          justfile = ''
            # Display devenv version
            version:
              devenv version
          '';
        };
      })
      (lib.genAttrs (builtins.attrNames config.scripts) (name:
        let
          script = config.scripts.${name};
        in
        {
          enable = script.just.enable;
          justfile = ''
            #${script.description}
            ${name}:
              ${name}
          '';
        }));


    enterShell =
      let
        commonJustfile = pkgs.writeTextFile {
          name = "justfile";
          text =
            lib.concatStringsSep "\n"
              (lib.mapAttrsToList (name: feature: feature.outputs.justfile) config.just.features);
        };
      in
      ''
        ln -sf ${builtins.toString commonJustfile} ./${config.just.commonFileName}

        echo
        echo "https://devenv.sh (version ${config.devenv.cliVersion}): Fast, Declarative, Reproducible, and Composable Developer Environments 💪💪"
        echo
        echo "Run 'just <recipe>' to get started"
        just --list
      '';
  };
}
