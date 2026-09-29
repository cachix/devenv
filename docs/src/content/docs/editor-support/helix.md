---
title: "Helix"
---

In your project root, put the following into your `.helix/languages.toml`:

```toml
[[language]]
name = "nix"
language-servers = ["devenv-lsp"]

[language-server.devenv-lsp]
command = "devenv"
args = ["lsp"]
```

This will make Helix use the `devenv lsp` for all Nix files.

Then enter `devenv shell` and run `hx`.

Most languages servers and code formatting tools provided by devenv are readily available to Helix, as it will just look for them in your `$PATH`.

To troubleshoot, inside your devenv shell, run `hx --health [<language_name>]`. This will print out which executables helix is configured to use for this language (or all languages known to Helix if no `language_name` is given), and which are correctly found.

For more information, have a look at [the Helix doc about languages](https://docs.helix-editor.com/lang-support.html).
