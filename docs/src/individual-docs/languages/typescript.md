# TypeScript

The TypeScript module provides the TypeScript compiler and, by default, the TypeScript language server.
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

Node.js is enabled by default when npm, pnpm, Yarn, or Corepack is enabled, but not for Bun or Deno.
The compiler and language server keep their own package dependencies, independent of the project's runtime.

See [JavaScript and TypeScript environments](/guides/javascript-typescript/) for version availability,
package customization, Deno, and mixed-language projects.

@AUTOGEN_OPTIONS@
