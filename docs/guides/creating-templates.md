The following guide describes how to extend devenv and provide templates for other users. It is possible to bootstrap `devenv.nix` with custom content, change defaults or even define custom devenv options.

## Templates

To provide a devenv template, add the following to your `devenv.nix` file:
```nix title="devenv.nix"
  templates.rust = {
    path = ./rust;
    description = "A simple Rust/Cargo project";
    welcomeText = ''
      # Simple Rust/Cargo Template
      ## Intended usage
      The intended usage is...

      ## More info
      - [Rust language](https://www.rust-lang.org/)
      - [Rust on the NixOS Wiki](https://wiki.nixos.org/wiki/Rust)
      - ...
    '';
  };

  templates.default = config.templates.rust;
}
```

A template has the following attributes:
- `description`: A one-line description of the template, in CommonMark syntax.
- `path`: The path of the directory to be copied. This directory can be created with `devenv init rust`.
- `welcomeText`: A block of markdown text to display when a user initializes a new project based on this template.

Template consumers can then use the following command to use the `default` template:
```
devenv init --template github:owner/repo
```

The reference to the Github repository is called a Flake reference. Some more examples:
- `github:owner/repo#rust`: The `rust` template.
- `github:owner/repo/some-branch`: The `default` template from the `some-branch` Git branch.
- `gitlab:owner/repo`: The `default` template from a Gitlab repository.

When testing you may also use a local directory path.

!!! note
    Devenv only supports a subset of the Flake reference specification. Notably it is currently not possible to specify a directory.

## Imports
Imports can be used to split `devenv.nix` when making more advanced templates.

```nix title="devenv.nix"
{ config, lib, ... }:
{
  imports = [ ./another-file.nix ];
}
```

It is also possible to import from an external repository. This method is recommended for implementation details that the template consumer is not supposed to modify.

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  templates:
    url: github:owner/repo
imports:
- templates
```

The specified repository is usually the same repository where the template itself is located.

The import always begins with the input name, but may include a directory (e.g. `templates/some/directory`). In the example above the import will look for a `devenv.nix` file in the Git root directory.

!!! note "Defaults"
    To set defaults prepend the value with `lib.mkDefault` so that the template consumer can change the value (e.g. `enable = lib.mkDefault true`). Lists are automatically merged and don't require `lib.mkDefault`. The template can specify a list of default packages and these are then merged with the users packages.

## Merging existing files

Templates cannot override existing files, however it is possible to implement custom logic to handle this.

First rename the template file (e.g. `.gitignore.devenv-template`) and then add the following to the `devenv.nix` file:

```nix
enterShell = ''
  if [ -f .gitignore.devenv-template ]; then
    cat .gitignore.devenv-template >> .gitignore
    rm .gitignore.devenv-template
  fi
'';
```

## Custom options

It is possible to extend devenv with additional modules and options. See <https://nix.dev/tutorials/module-system/deep-dive.html>
