{ pkgs, lib, config, ... }:

{
  options.difftastic.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Integrat difftastic into git: https://difftastic.wilfred.me.uk/";
  };

  config = lib.mkIf config.difftastic.enable {
    packages = [ pkgs.difftastic ];

    env.GIT_EXTERNAL_DIFF = "difft";
  };
}
