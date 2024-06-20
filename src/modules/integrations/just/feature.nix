{ config, name, lib, pkgs, ... }: {
  options = {
    enable = lib.mkEnableOption "Enable this feature";
    justfile = lib.mkOption {
      type = lib.types.either lib.types.str lib.types.path;
      description = ''
        The justfile representing this feature.
      '';
      apply = x:
        if builtins.isPath x then x else
        pkgs.writeTextFile {
          name = "${name}.just";
          text = x;
        };
    };
    outputs.justfile = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      description = ''
        The justfile code for importing this feature's justfile.

        See https://just.systems/man/en/chapter_53.html
      '';
      default =
        if config.enable
        then "import '${builtins.toString config.justfile}'"
        else "";
    };
  };
}
