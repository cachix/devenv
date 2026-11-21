{ config, pkgs, ... }:
{
  languages.typescript = {
    enable = true;
    nodejs.package = pkgs.nodejs_24;
    pnpm = {
      enable = true;
      install.enable = true;
    };
  };

  assertions = [
    {
      assertion = !config.languages.javascript.enable && config.languages.typescript.nodejs.enable;
      message = "TypeScript with pnpm must enable Node.js without enabling JavaScript";
    }
  ];
}
