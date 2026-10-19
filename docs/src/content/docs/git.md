---
title: Git
---

Configure Git for a development environment with `git.config`:

```nix
{
  git.config = {
    merge.conflictStyle = "diff3";
    merge.tool = "vimdiff";
    diff.algorithm = "histogram";
  };
}
```

Sections and subsections use nested attribute sets. Values may be strings,
booleans, numbers, or lists for repeated keys. These settings apply to Git commands
inside the devenv environment. Git still reads your existing configuration, but
these settings take precedence over configuration files, including repository
configuration. Explicit `git -c key=value` options take precedence over devenv.

The implementation uses `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_0`, and
`GIT_CONFIG_VALUE_0` to include a generated configuration file. Do not configure
those environment variables separately. Generated files are stored in the Nix
store, so configuration must not contain secrets.

Use `git.attributes` to select merge drivers, diff drivers, and other file-specific
behavior. For example, this configuration registers a custom merge driver and
selects it for generated text files:

```nix
{
  git.config.merge.keepCurrent = {
    name = "Keep the current version";
    driver = "true";
  };
  git.attributes = [
    "generated/*.txt merge=keepCurrent"
    "*.png binary"
  ];
}
```

Attribute rules are written to a generated file selected by `core.attributesFile`.
When configured, this replaces your global attributes file for the environment.
Repository `.gitattributes` files and `.git/info/attributes` retain their higher
precedence. Devenv does not modify those files or your Git configuration files.

The defaults are empty and leave Git's configuration and attributes unchanged.
Install any tools referenced by your settings through `packages`.
