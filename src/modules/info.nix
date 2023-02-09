{ lib, config, ... }:

let
  renderSection = section: lib.concatStringsSep "\n" (builtins.map (entry: "- ${entry}") section);
in
{
  options = {
    info = lib.mkOption {
      type = lib.types.lines;
      internal = true;
    };

    infoSections = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.str);
      default = { };
      description =
        "Information about the environment";
    };
  };


  config = {
    info = lib.concatStringsSep "\n\n" (lib.mapAttrsToList (name: section: if (section != [ ]) then "# ${name}\n${renderSection section}" else "") config.infoSections);
  };
}
