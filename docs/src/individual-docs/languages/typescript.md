# TypeScript

The TypeScript module provides the TypeScript compiler, Node.js, and the TypeScript language server by default.
Configure Node.js, Bun, Deno, npm, pnpm, Yarn, or Corepack directly under `languages.typescript`;
you do not need to enable `languages.javascript`.

```nix
{
  languages.typescript = {
    enable = true;
    bun.enable = true;
    bun.install.enable = true;
  };
}
```

Package managers are disabled by default. Enabling Bun or Deno adds them alongside Node.js.
Set `languages.typescript.nodejs.enable = false` if you do not want the project Node.js runtime.
The compiler and language server keep their own package dependencies, independent of the project's runtime.

See [JavaScript and TypeScript environments](/guides/javascript-typescript/) for version availability,
package customization, Deno, and mixed-language projects.

@AUTOGEN_OPTIONS@
