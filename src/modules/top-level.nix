{ config, pkgs, lib, ... }:
let
  types = lib.types;
  mkNakedShell = pkgs.callPackage ./mkNakedShell.nix { };
  # Returns a list of all the entries in a folder
  listEntries = path:
    map (name: path + "/${name}") (builtins.attrNames (builtins.readDir path));

  drvOrPackageToPaths = drvOrPackage:
    if drvOrPackage ? outputs then
      builtins.map (output: drvOrPackage.${output}) drvOrPackage.outputs
    else
      [ drvOrPackage ];
  profile = pkgs.buildEnv {
    name = "devenv-profile";
    paths = lib.flatten (builtins.map drvOrPackageToPaths config.packages);
    ignoreCollisions = true;
  };

  failedAssertions = builtins.map (x: x.message) (builtins.filter (x: !x.assertion) config.assertions);

  performAssertions =
    let
      formatAssertionMessage = message:
        let
          lines = lib.splitString "\n" message;
        in
        "- ${lib.concatStringsSep "\n  " lines}";
    in
    if failedAssertions != [ ]
    then
      throw ''
        Failed assertions:
        ${lib.concatStringsSep "\n" (builtins.map formatAssertionMessage failedAssertions)}
      ''
    else lib.id;
in
{
  options = {
    env = lib.mkOption {
      type = types.lazyAttrsOf types.anything;
      description = "Environment variables to be exposed inside the developer environment.";
      default = { };
    };

    name = lib.mkOption {
      type = types.nullOr types.str;
      description = "Name of the project.";
      default = null;
    };

    enterShell = lib.mkOption {
      type = types.lines;
      description = "Bash code to execute when entering the shell.";
      default = "";
    };

    packages = lib.mkOption {
      type = types.listOf types.package;
      description = "A list of packages to expose inside the developer environment. Search available packages using ``devenv search NAME``.";
      default = [ ];
    };

    shell = lib.mkOption {
      type = types.package;
      internal = true;
    };

    ci = lib.mkOption {
      type = types.listOf types.package;
      internal = true;
    };

    ciDerivation = lib.mkOption {
      type = types.package;
      internal = true;
    };

    assertions = lib.mkOption {
      type = types.listOf types.unspecified;
      internal = true;
      default = [ ];
      example = [{ assertion = false; message = "you can't enable this for that reason"; }];
      description = ''
        This option allows modules to express conditions that must
        hold for the evaluation of the configuration to succeed,
        along with associated error messages for the user.
      '';
    };
  };

  imports = [
    ./info.nix
    ./processes.nix
    ./scripts.nix
    ./update-check.nix
    ./containers.nix
  ]
  ++ (listEntries ./languages)
  ++ (listEntries ./services)
  ++ (listEntries ./integrations)
  ;

  config = {
    # TODO: figure out how to get relative path without impure mode
    env.DEVENV_ROOT = builtins.getEnv "PWD";
    env.DEVENV_DOTFILE = config.env.DEVENV_ROOT + "/.devenv";
    env.DEVENV_STATE = config.env.DEVENV_DOTFILE + "/state";
    env.DEVENV_PROFILE = profile;

    enterShell = ''
      export PS1="\[\e[0;34m\](devenv)\[\e[0m\] ''${PS1-}"

      # set path to locales on non-NixOS Linux hosts
      ${lib.optionalString (pkgs.stdenv.isLinux && (pkgs.glibcLocalesUtf8 != null)) ''
        if [ -z "''${LOCALE_ARCHIVE-}" ]; then
          export LOCALE_ARCHIVE=${pkgs.glibcLocalesUtf8}/lib/locale/locale-archive
        fi
      ''}

      # note what environments are active, but make sure we don't repeat them
      if [[ ! "''${DIRENV_ACTIVE-}" =~ (^|:)"$PWD"(:|$) ]]; then
        export DIRENV_ACTIVE="$PWD:''${DIRENV_ACTIVE-}"
      fi

      # devenv helper
      if [ ! type -p direnv &>/dev/null && -f .envrc ]; then
        echo "You have .envrc but direnv command is not installed."
        echo "Please install direnv: https://direnv.net/docs/installation.html"
      fi

      mkdir -p .devenv
      rm -f .devenv/profile
      ln -s ${profile} .devenv/profile
    '';

    shell = performAssertions (
      mkNakedShell {
        name = "devenv-shell";
        env = config.env;
        profile = profile;
        shellHook = config.enterShell;
      }
    );

    infoSections."env" = lib.mapAttrsToList (name: value: "${name}: ${toString value}") config.env;
    infoSections."packages" = builtins.map (package: package.name) (builtins.filter (package: !(builtins.elem package.name (builtins.attrNames config.scripts))) config.packages);

    ci = [ config.shell.inputDerivation ];
    ciDerivation = pkgs.runCommand "ci" { } ("ls " + toString config.ci + " && touch $out");
  };
}
