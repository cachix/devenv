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
      mergeTool = "mergiraf";
      mergeDriver.mergiraf = {
        name = "mergiraf";
        driver = "mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L";
      };
      # mergiraf requires diff3 conflict style to reconstruct the base revision
      conflictStyle = "diff3";
    };
  };
}
