{ pkgs, config, lib, ... }:

let
  cfg = config.direnv;
  inherit (lib) types;
in
{
  options.direnv = {
    watchFiles = lib.mkOption {
      description = lib.mdDoc "A list of files to watch for changes and then reload the direnv environment";
      # TODO: best type here? any way to exclude defaults?
      # TODO: regex support?
      type = types.listOf types.str;
      default = [ ];
    };

    watchfiles = lib.mkOption {
      internal = true;
      type = types.package;
    };
  };

  config.direnv.watchfiles = pkgs.writeText "watchfiles.txt" (lib.concatStringsSep "\n" cfg.watchFiles);
}
