{
  config,
  lib,
  pkgs,
  ...
}:
{
  languages.typescript = {
    enable = true;
    directory = "frontend";
    bun = {
      enable = true;
      install.enable = true;
    };
  };

  assertions = [
    {
      assertion = !config.languages.javascript.enable && !config.languages.typescript.nodejs.enable;
      message = "TypeScript with Bun must not enable the JavaScript or Node.js presets";
    }
    {
      assertion = !(lib.elem pkgs.nodejs-slim config.packages);
      message = "TypeScript with Bun must not add the project Node.js package";
    }
  ];
}
