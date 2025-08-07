{ lib
, config
, pkgs
, ...
}:

let
  types = lib.types;
  scriptType = types.submodule (
    { config, name, ... }:
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
          defaultText = lib.literalExpression "pkgs.bash";
        };
        binary = lib.mkOption {
          type = types.nullOr types.str;
          description = "Override the binary name from the default `package.meta.mainProgram`";
          default = null;
        };
        description = lib.mkOption {
          type = types.str;
          description = "Description of the script.";
          default = "";
        };
        packages = lib.mkOption {
          type = types.listOf types.package;
          description = "Packages to be available in PATH when the script runs.";
          default = [ ];
          defaultText = lib.literalExpression "[]";
        };
        scriptPackage = lib.mkOption {
          internal = true;
          type = types.package;
        };
      };

      config.scriptPackage =
        let
          binary =
            if config.binary != null
            then "${pkgs.lib.getBin config.package}/bin/${config.binary}"
            else pkgs.lib.getExe config.package;
        in
        lib.hiPrioSet (
          pkgs.runCommand name
            {
              buildInputs = config.packages;
              text = ''
                #!${binary}
                ${config.exec}
              '';
              passAsFile = [ "text" ];
              meta.mainProgram = name;
            } ''
            target="$out/bin/${name}"
            mkdir -p "$(dirname "$target")"
            mv "$textPath" "$target"
            chmod +x "$target"
          ''
        );
    }
  );

  renderInfoSection = name: script:
    "${name}${lib.optionalString (script.description != "") ": ${script.description}"} ${script.scriptPackage}";
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
    packages = lib.mapAttrsToList (_: script: script.scriptPackage) config.scripts;

    infoSections."scripts" = lib.mapAttrsToList renderInfoSection config.scripts;
  };
}
