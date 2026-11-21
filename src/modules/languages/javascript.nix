{ config, lib, ... }:

let
  cfg = config.languages.javascript;
in
{
  imports = [
    ../lib/javascript.nix
    (lib.mkAliasOptionModule [ "languages" "javascript" "package" ] [ "languages" "javascript" "nodejs" "package" ])
  ];

  options.languages.javascript.enable = lib.mkEnableOption "tools for JavaScript development";

  config = lib.mkIf cfg.enable {
    changelogs = [
      {
        date = "2026-05-12";
        title = "languages.javascript.npm uses 'clean-install' when a lockfile exists";
        description = ''
          When `languages.javascript.npm.install.enable = true` and a `package-lock.json` is present, devenv now runs `npm clean-install` instead of `npm install`. This installs the exact versions from the lockfile and avoids silent semver bumps. Falls back to `npm install` when no lockfile exists.
        '';
      }
      {
        date = "2026-05-01";
        title = "languages.javascript.nodejs.enable makes Node.js optional";
        description = ''
          Added `languages.javascript.nodejs.enable` (defaults to `true`) so Node.js can be opted out when only `bun` or another runtime is needed. Enabling `npm`, `pnpm`, `yarn`, `corepack`, or `lsp` requires `nodejs.enable = true`.
        '';
      }
    ];

    assertions = [
      {
        assertion = cfg.lsp.enable -> cfg.nodejs.enable;
        message = "languages.javascript.lsp.enable requires languages.javascript.nodejs.enable = true;";
      }
    ];
  };
}
