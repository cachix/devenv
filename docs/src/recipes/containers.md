## Container patterns

### Exclude packages from a container

```nix title="devenv.nix"
{ pkgs, lib, config, ... }: {
  packages = [
    pkgs.git
  ] ++ lib.optionals (!config.container.isBuilding) [
    pkgs.haskell-language-server
  ];
}
```
