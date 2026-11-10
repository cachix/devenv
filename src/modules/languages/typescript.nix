{ pkgs, config, lib, ... }:

let
  cfg = config.languages.typescript;
in
{
  imports = [ ../lib/javascript.nix ];

  options.languages.typescript.enable = lib.mkEnableOption "tools for TypeScript development";

  config = lib.mkIf cfg.enable {
    packages = [ pkgs.typescript ];

    changelogs = [
      {
        date = "2026-11-10";
        title = "TypeScript supports JavaScript runtimes and package managers directly";
        description = ''
          Configure `nodejs`, `bun`, `deno`, `npm`, `pnpm`, `yarn`, and `corepack` under `languages.typescript` without enabling `languages.javascript`.
          TypeScript still provides its compiler and language server by default. Node.js is enabled by default only when npm, pnpm, Yarn, or Corepack is enabled.
        '';
      }
    ];
  };
}
