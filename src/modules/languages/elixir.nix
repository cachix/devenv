{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;
in
{
  options.languages.elixir = {
    enable = lib.mkEnableOption "tools for Elixir development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Elixir to use.";
      default = pkgs.elixir;
      defaultText = lib.literalExpression "pkgs.elixir";
    };
  };

  config = lib.mkIf cfg.enable
    {
      packages = with pkgs; [
        cfg.package
        elixir_ls
      ];
    };
}
