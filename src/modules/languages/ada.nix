{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.ada;
in
{
  options.languages.ada = {
    enable = lib.mkEnableOption "Tools for Ada Development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gnatPackages.gnat;
      defaultText = "pkgs.gnatPackages.gnat";
      description = "GNAT Compiler package used to build Ada projects.";
    };

    gprbuild = {
      enable = lib.mkEnableOption "Ada Multi-language extensible build tool.Alire Package Manager" // {
        default = cfg.enable;
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.gnatPackages.gprbuild;
        defaultText = "pkgs.gnatPackages.gprbuild";
        description = "GPRbuild package used to build Ada and multi-language projects.";
      };
    };

    alire = {
      enable = lib.mkEnableOption "Alire Package Manager";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.alire;
        defaultText = "pkgs.alire";
        description = "Alire is a source-based package manager for the Ada and SPARK programming languages.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      [ cfg.package ]
      ++ lib.optional cfg.gprbuild.enable cfg.gprbuild.package
      ++ lib.optional cfg.alire.enable cfg.alire.package
    ;

    # Without this, alire produces errors when linking:
    #  cannot find Scrt1.o: No such file or directory
    #  cannot find crti.o: No such file or directory
    #  cannot find -ldl: No such file or directory
    #  cannot find -lc: No such file or directory
    #  cannot find crtn.o: No such file or directory
    env = lib.optionalAttrs cfg.alire.enable {
      LIBRARY_PATH = "${pkgs.glibc}/lib:$LIBRARY_PATH";
    };
  };
}
