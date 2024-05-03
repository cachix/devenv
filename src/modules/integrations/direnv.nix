{ pkgs, config, lib, ... }:

let
  cfg = config.direnv;
in
{
  options.direnv = {
    watchFiles = lib.mkOption {
      description = lib.mdDoc "Extra files to watch for changes and reload the direnv environment";
      # TODO: best type here? any way to exclude defaults?
      # TODO: regex support?
      type = lib.types.listOf lib.types.str;
      default = [ ];
    };

    watchfiles = lib.mkOption {
      internal = true;
      type = lib.types.package;
    };
  };

  config = {
    direnv.watchfiles = pkgs.writeText "watchfiles.txt" (lib.concatStringsSep "\n" cfg.watchFiles);
  };
}
