{ pkgs, lib, config, ... }:

let
  types = lib.types;
  scriptType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "Bash code to execute when the script is run.";
      };
      description = lib.mkOption {
        type = types.str;
        description = "Description of the script.";
        default = "";
      };
    };
  });
  # lib.hiPrioSet: prioritize scripts over plain packages
  toPackage = name: script: lib.hiPrioSet (pkgs.writeShellScriptBin name script.exec);
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
