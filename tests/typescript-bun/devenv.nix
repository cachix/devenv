{
  config,
  lib,
  pkgs,
  ...
}:
{
  languages.typescript = {
    enable = true;
    nodejs.enable = false;
    directory = "frontend";
    bun = {
      enable = true;
      install.enable = true;
    };
  };

  assertions = [
    {
      assertion = !config.languages.javascript.enable && !config.languages.typescript.nodejs.enable;
      message = "TypeScript with Bun must respect the Node.js opt-out without enabling JavaScript";
    }
    {
      assertion = !(lib.elem pkgs.nodejs-slim config.packages);
      message = "An explicit Node.js opt-out must not add the project Node.js package";
    }
  ];
}
