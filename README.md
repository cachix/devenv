# dev.nix - Developer environments

Given `dev.nix`:

```nix
{ pkgs }:

{
  env.FOO = true;

  imports = [ ./frontend/dev.nix ];

  packages = [ pkgs.git ];

  processes.<name>.cmd = "lala";
}
```

And `dev.yaml`:

```yaml
inputs.nixpkgs.url = ...
```

## Commands

``dev.nix shell``: make `packages` available and export `env` variables

``dev.nix up``: start processes

``dev.nix init``: generate `dev.nix`, `dev.yaml` and `.envrc`

## Issues

- if we generate flake.nix, errors will come from the wrong file. We should instead import dev.nix!

## TODO

- cachix integration
- pre-commit.nix
- build containters out of the processes