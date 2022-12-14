{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;
in
{
  options.languages.elixir = {
    enable = lib.mkEnableOption "Enable tools for Elixir development.";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Elixir to use";
      default = pkgs.elixir;
      defaultText = "pkgs.elixir";
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
