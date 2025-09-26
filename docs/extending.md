# Extending devenv

Projects can encode their best practices by creating custom modules with opinionated defaults in a central repository:

```nix title="devenv.nix"
{ lib, config, pkgs, ... }: {
  options.myproject = {
    languages.rust.enable = lib.mkEnableOption "Rust development stack";
    services.database.enable = lib.mkEnableOption "Database services";
  };

  config = {
    packages = lib.mkIf config.myproject.languages.rust.enable [
      pkgs.cargo-watch
    ];

    languages.rust = lib.mkIf config.myproject.languages.rust.enable {
      enable = true;
      channel = "nightly";
    };

    services.postgres = lib.mkIf config.myproject.services.database.enable {
      enable = true;
      initialScript = "CREATE DATABASE myapp;";
    };
  };
}
```

Once you have your best practices module defined, you can start using it in new projects:

```yaml title="devenv.yaml"
inputs:
  myproject:
    url: github:myorg/devenv-myproject
    flake: false
imports:
- myproject
```

This automatically includes your centrally managed module. Since options default to `false`, you'll need to enable them per project.

!!! tip "Profiles"

    You can enable common defaults globally and use [profiles](profiles.md) to activate additional components on demand.

## Module Replacement

You can replace existing devenv modules using the `disabledModules` mechanism. This allows you to override built-in behavior or provide custom implementations.

```nix
{ pkgs, lib, config, ... }: {
  # Disable the original module
  disabledModules = [ "languages/rust.nix" ];

  options.languages.rust = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };
  };

  config = lib.mkIf config.languages.rust..enable {
    packages = [ pkgs.rustc ];
  };
}
```
