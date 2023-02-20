{ pkgs, lib, config, ... }:

let
  cfg = config.toolkits.cuda;
in
{
  options.toolkits.cuda = {
    enable = lib.mkEnableOption "CUDA toolkit";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of cuda toolkit to use.";
      default = pkgs.cudatoolkit;
      defaultText = lib.literalExpression "pkgs.cudatoolkit";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    # Required for pytorch
    env.LD_LIBRARY_PATH = lib.mkIf pkgs.stdenv.isLinux (
      lib.makeLibraryPath [
        pkgs.gcc-unwrapped.lib
        pkgs.linuxPackages.nvidia_x11
      ]
    );
    env.CUDA_HOME = "${cfg.package}";
    env.CUDA_PATH = "${cfg.package}";
  };
}
