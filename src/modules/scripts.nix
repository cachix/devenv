{ lib
, config
, pkgs
, ...
}:

let
  types = lib.types;
  scriptType = types.submodule (
    { config, ... }:
    {
      options = {
        exec = lib.mkOption {
          type = types.str;
          description = "Shell code to execute when the script is run.";
        };
        package = lib.mkOption {
          type = types.package;
          description = "The package to use to run the script.";
          default = pkgs.bash;
        };
        binary = lib.mkOption {
          type = types.str;
          description = "Override the binary name if it doesn't match package name";
          default = config.package.pname;
        };
        description = lib.mkOption {
          type = types.str;
          description = "Description of the script.";
          default = "";
        };
      };
    }
  );

  # lib.hiPrioSet: prioritize scripts over plain packages
  toPackage =
    name: script:
    lib.hiPrioSet (
      pkgs.writeScriptBin name ''
        #!${pkgs.lib.getBin script.package}/bin/${script.binary}
        ${script.exec}
      ''

    );
in
{
  options = {
    scripts = lib.mkOption {
      type = types.attrsOf scriptType;
      default = { };
      description = "A set of scripts available when the environment is active.";
    };
  };

  config = {
    packages = lib.mapAttrsToList toPackage config.scripts;

    # TODO: show scripts path
    infoSections."scripts" = lib.mapAttrsToList (name: script: name) config.scripts;
  };
}
