# JavaScript

The JavaScript module provides Node.js and the TypeScript/JavaScript language server by default.
Enable npm, pnpm, Yarn, Bun, Deno, or Corepack as needed.

```nix
{
  languages.javascript = {
    enable = true;
    pnpm.enable = true;
    pnpm.install.enable = true;
  };
}
```

Enabling Bun or Deno does not disable the existing Node.js default.
For a TypeScript project, the same tooling options are available directly under `languages.typescript`.
See [JavaScript and TypeScript environments](/guides/javascript-typescript/).

@AUTOGEN_OPTIONS@
