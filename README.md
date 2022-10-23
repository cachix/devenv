# dev.nix - Developer environments

Given `dev.nix`:

```nix
{ pkgs, ... }:

{
  env.FOO = true;

  include = [ ./frontend/dev.nix ];

  enter = ''
    echo hello
  '';

  packages = [ pkgs.git ];

  processes."<name>".cmd = "lala";
}
```

And `dev.yaml`:

```yaml
inputs:
  - nixpkgs:
     - url: github:NixOS/nixpkgs/nixos-22.05
```

## Commands

``dev.nix shell``: make `packages` available and export `env` variables

``dev.nix up``: start processes

``dev.nix init``: generate `dev.nix`, `dev.yaml` and `.envrc`

``dev.nix update``: bump `dev.lock`

``dev.nix ci``: build all packages and push them to Cachix

## TODO

- integrations via flakes
- postgres module
- cachix integration: when composing as well
- pre-commit.nix integration
- build containters out of the processes