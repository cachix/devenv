# Extending devenv

Teams can encode their best practices by creating custom modules with opinionated defaults in a central repository:

```nix title="devenv.nix"
{ lib, config, pkgs, ... }: {
  options.myteam = {
    languages.rust.enable = lib.mkEnableOption "Rust development stack";
    services.database.enable = lib.mkEnableOption "Database services";
  };

  config = {
    packages = lib.mkIf config.myteam.languages.rust.enable [
      pkgs.cargo-watch
    ];

    languages.rust = lib.mkIf config.myteam.languages.rust.enable {
      enable = true;
      channel = "nightly";
    };

    services.postgres = lib.mkIf config.myteam.services.database.enable {
      enable = true;
      initialScript = "CREATE DATABASE myapp;";
    };
  };
}
```

Once you have your team module defined, you can start using it in new projects:

```yaml title="devenv.yaml"
inputs:
  myteam:
    url: github:myorg/devenv-myteam
    flake: false
imports:
- myteam
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
    packages = [ pkgs.python3 ];
    enterShell = "echo 'Custom Python environment'";
  };
}
```
