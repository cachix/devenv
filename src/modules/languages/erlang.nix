{ pkgs, config, lib, ... }:

let
  cfg = config.languages.erlang;
  # There's no override available
  rebar3 = pkgs.rebar3.overrideAttrs (_: {
    buildInputs = [ cfg.package ];
  });
in
{
  options.languages.erlang = {
    enable = lib.mkEnableOption "tools for Erlang development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which Erlang package to use.";
      default = pkgs.erlang;
      defaultText = lib.literalExpression "pkgs.erlang";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.erlang-language-platform
      rebar3
    ];
  };
}
