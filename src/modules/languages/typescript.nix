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
        when = cfg.enable;
        description = ''
          Configure `nodejs`, `bun`, `deno`, `npm`, `pnpm`, `yarn`, and `corepack` under `languages.typescript` without enabling `languages.javascript`.
          TypeScript now enables Node.js by default alongside its compiler and language server, matching JavaScript. Package managers remain disabled by default.
          Set `languages.typescript.nodejs.enable = false` to keep a tooling-only environment or use Bun or Deno without the project Node.js runtime.
        '';
      }
    ];
  };
}
