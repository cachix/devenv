# Java

The Java module adds a JDK to your shell, with optional Maven, Gradle, and the Eclipse JDT Language Server.
devenv sets `JAVA_HOME` to the JDK you select.

## Language Server

The language server can run on a different JDK from your project.
There are two ways to tell it which project JDK to use:

- `java.home` sets one default project JDK in Neovim. VS Code deprecates this setting.
- `java.configuration.runtimes` maps Java versions, such as `JavaSE-17`, to installed JDKs.
  Use this for multiple JDKs or explicit version mappings.

Maven and Gradle projects still set their Java language version in `pom.xml` or `build.gradle`.

### Gradle projects

The Eclipse JDT Language Server, which powers Java support in VS Code, Neovim, and most other editors,
imports a Gradle project by running Gradle.
Without a project JDK configured, it may run Gradle on the server's JDK.

Set `java.import.gradle.java.home` to the project JDK to run the import on the same JDK as your shell.

### VS Code

The [Java extension](https://marketplace.visualstudio.com/items?itemName=redhat.java) bundles its own language server.
It normally detects project JDKs from `JAVA_HOME` and the build configuration.
Set `languages.java.lsp.enable = false` if you only use this extension.

Generate the workspace settings from `devenv.nix`, so the JDK path stays in sync with your project:

```nix
{ config, lib, ... }:
let
  jdk = config.languages.java.jdk.package;
  major = lib.versions.major jdk.version;
in
{
  files.".vscode/settings.json".json = {
    "java.configuration.runtimes" = [
      {
        name = "JavaSE-${major}";
        path = jdk.home;
        default = true;
      }
    ];
    "java.import.gradle.java.home" = jdk.home;
  };
}
```

The extension's bundled runtime starts the server; the runtime mapping registers the project JDK.
See Microsoft's [Configure Runtime for Projects](https://code.visualstudio.com/docs/java/java-project#_configure-runtime-for-projects).

### Neovim (LazyVim)

Save this as `.lazy.lua` in your project root. LazyVim loads it as a
[project-local configuration](https://lazy.folke.io/configuration).

```lua
local profile = vim.env.DEVENV_PROFILE
if not profile then
  return {}
end

local java = {
  cmd = { profile .. "/bin/jdtls" },
  settings = {
    java = {
      home = vim.env.JAVA_HOME,
      import = { gradle = { java = { home = vim.env.JAVA_HOME } } },
    },
  },
}

return {
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        jdtls = vim.tbl_extend("force", java, { mason = false }),
      },
    },
  },
  {
    "mfussenegger/nvim-jdtls",
    optional = true,
    opts = java,
  },
}
```

@AUTOGEN_OPTIONS@
