---
title: "Language Server"
---

`devenv lsp` starts a bundled [nixd](https://github.com/nix-community/nixd) language server pre-configured for your `devenv.nix`. It provides autocomplete, hover documentation, and go to definition in any editor that supports LSP.

## Usage

```sh
$ devenv lsp
```

The command assembles your devenv configuration, generates the appropriate nixd settings (nixpkgs expression and devenv options), and then replaces itself with the `nixd` process. Your editor communicates with it over stdio.

## Editor setup

Most editors can be configured to use `devenv lsp` as the Nix language server. Point your editor's LSP client at `devenv lsp` as the server command.

If you need the generated nixd configuration for manual editor setup:

```sh
$ devenv lsp --print-config
```

This prints the nixd JSON configuration and exits without starting the server.
