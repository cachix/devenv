{ pkgs, lib, config, ... }:

{
  options.delta.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Integrate delta into git: https://dandavison.github.io/delta/.";
  };

  config = lib.mkIf config.delta.enable {
    packages = [ pkgs.delta ];

    env.GIT_PAGER = "delta";
  };
}
