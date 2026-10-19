{ pkgs, lib, config, ... }:

{
  options.mergiraf.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Integrate mergiraf as git merge driver: https://mergiraf.org/";
  };

  config = lib.mkIf config.mergiraf.enable {
    packages = [ pkgs.mergiraf ];

    git = {
      config.merge = {
        mergiraf = {
          name = "mergiraf";
          driver = "mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L";
        };
        # Mergiraf needs the base revision when solving existing conflicts.
        conflictStyle = lib.mkDefault "diff3";
      };
      attributes = [ "* merge=mergiraf" ];
    };
  };
}
