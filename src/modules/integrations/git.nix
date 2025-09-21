{ pkgs, config, lib, ... }:

{
  options.git = {
    root = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Git repository root path. This field is populated automatically in devenv 1.10 and newer.";
      default = null;
    };
  };
}
