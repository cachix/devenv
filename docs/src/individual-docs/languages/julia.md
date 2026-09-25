Enabling Julia also installs [Fatou](https://fatou.dev/), a language server,
formatter, and linter for Julia:

```nix
languages.julia.enable = true;
```

Configure your editor to run `fatou lsp` for Julia files. See Fatou's
[editor setup guide](https://fatou.dev/guide/editors.html) for details.

To disable the language server package:

```nix
languages.julia.lsp.enable = false;
```

Use `languages.julia.lsp.package` to select a different language server package.

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
