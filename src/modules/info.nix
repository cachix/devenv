{ lib, config, ... }:

{
  options = {
    info = lib.mkOption {
      type = lib.types.lines;
      internal = true;
    };
  };

  # TODO: show scripts path
  # TODO: make this pluggable so any option can be shown

  config.info = ''
    # env
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: value: "- ${name}: ${toString value}") config.env)}

    # packages
    ${lib.concatStringsSep "\n" (builtins.map (package: "- ${package.name}") (builtins.filter (package: !(builtins.elem package.name (builtins.attrNames config.scripts))) config.packages))}

    # scripts
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: script: "- ${name}") config.scripts)}

    # processes
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: process: "- ${name}: ${process.exec}") config.processes)}
  '';
}
