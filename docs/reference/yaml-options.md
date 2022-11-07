# devenv.yaml options

| Key                        | Value                                                         |
| -------------------------- | ------------------------------------------------------------- |
| inputs                     |                                                               |
| inputs.<name>              | Name used when passing the input to your devenv.nix function. |
| inputs.<name>.url          | XXX                                                           |
| inputs.<name>.flake        | XXX                                                           |

An Example:

```yaml
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-22.05
  nixpkgs-2:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
imports:
  - ./relative/path
  - nixpkgs-2
```

document: imports

## URLS

- github:NixOS/nixpkgs/master
- github:NixOS/nixpkgs/master?rev=238b18d7b2c8239f676358634bfb32693d3706f3
- github:foo/bar?dir=subdir
- git+https://git.somehost.tld/user/path?ref=branch&rev=fdc8ef970de2b4634e1b3dca296e1ed918459a9e
- path:/path/to/repo

## URI schemas are types

path
git
github
tarball
mercurial
file
sourcehut
