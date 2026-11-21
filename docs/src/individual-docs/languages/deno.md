# Deno

Deno can be enabled through either `languages.javascript.deno.enable` or `languages.typescript.deno.enable`.
See [JavaScript and TypeScript environments](/guides/javascript-typescript/) for examples and version availability.

The standalone `languages.deno` options remain supported for existing configurations.
All entry points use the same project-local installation and cache directories.
If more than one entry point is enabled, their Deno packages must match.

@AUTOGEN_OPTIONS@
