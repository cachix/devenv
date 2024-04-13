{ pkgs, config, lib, ... }:

let
  cfg = config.languages.erlang;
in
{
  options.languages.erlang = {
    enable = lib.mkEnableOption "tools for Erlang development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Erlang to use.";
      default = pkgs.erlang;
      defaultText = lib.literalExpression "pkgs.erlang";
    };
  };

  config = lib.mkIf cfg.enable
    {
      packages = [
        cfg.package
        pkgs.erlang-ls
        pkgs.rebar3
      ];
    };
}
