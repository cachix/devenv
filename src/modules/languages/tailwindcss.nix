{ pkgs, config, lib, ... }:

let
  cfg = config.languages.tailwindcss;
in
{
  options.languages.tailwindcss = {
    enable = lib.mkEnableOption "tools for Tailwind CSS development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.tailwindcss_4;
      defaultText = lib.literalExpression "pkgs.tailwindcss_4";
      description = "The Tailwind CSS package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Tailwind CSS Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.tailwindcss-language-server;
        defaultText = lib.literalExpression "pkgs.tailwindcss-language-server";
        description = "The Tailwind CSS language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
