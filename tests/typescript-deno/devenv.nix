{ config, ... }:
{
  languages.typescript = {
    enable = true;
    deno.enable = true;
  };

  assertions = [
    {
      assertion = !config.languages.javascript.enable && config.languages.typescript.nodejs.enable;
      message = "TypeScript with Deno must keep the Node.js default without enabling JavaScript";
    }
  ];
}
