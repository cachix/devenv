---
title: "Neovim"
---

Start Neovim from inside your devenv environment so that the language servers, formatters and other tools provided by devenv are found in `$PATH`:

```shell-session
$ devenv shell
$ nvim
```

If you use [auto-activation](../auto-activation.mdx), Neovim started from the project directory picks up the environment automatically.

## Language server

`devenv lsp` provides completion and diagnostics for `devenv.nix`. With Neovim 0.11 or newer, add this to your `init.lua`:

```lua
vim.lsp.config("devenv", {
  cmd = { "devenv", "lsp" },
  filetypes = { "nix" },
  root_markers = { "devenv.nix", "devenv.yaml" },
})
vim.lsp.enable("devenv")
```

## Running tasks with overseer.nvim

[overseer.nvim](https://github.com/stevearc/overseer.nvim) is a task runner for Neovim. It ships with a built-in `devenv` template that lists every devenv [task](../tasks.mdx) defined in the nearest `devenv.nix` and runs it with `devenv tasks run <name>`. No extra configuration is needed.

Install the plugin and call `setup()`. With `vim.pack`, available in Neovim 0.12 and newer:

```lua
vim.pack.add({ "https://github.com/stevearc/overseer.nvim" })

require("overseer").setup()
```

Then run `:OverseerRun` and pick one of the `devenv <task>` entries. The `devenv` executable must be in your `$PATH`, so start Neovim from `devenv shell` or use [auto-activation](../auto-activation.mdx).

To run other devenv commands, such as starting [processes](../processes.mdx), use `:OverseerShell devenv up`.
