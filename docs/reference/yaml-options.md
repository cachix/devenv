
| Key                        | Value                                                                         |
| -------------------------- | ----------------------------------------------------------------------------- |
| inputs                     | Defaults to `inputs.nixpkgs.url: github:NixOS/nixpkgs/nixpkgs-unstable`.      |
| inputs.&lt;name&gt;        | Identifier name used when passing the input in your ``devenv.nix`` function.  |
| inputs.&lt;name&gt;.url    | URI specification of the input, see below for possible values.                |
| inputs.&lt;name&gt;.flake  | Does the input contain ``flake.nix`` or ``devenv.nix``. Defaults to ``true``. |
| imports                    | A list of relative paths or references to inputs to import ``devenv.nix``.    |

## inputs.&lt;name&gt;.url

- github:NixOS/nixpkgs/master
- github:NixOS/nixpkgs/master?rev=238b18d7b2c8239f676358634bfb32693d3706f3
- github:foo/bar?dir=subdir
- git+ssh://git@github.com/NixOS/nix?ref=v1.2.3
- git+https://git.somehost.tld/user/path?ref=branch&rev=fdc8ef970de2b4634e1b3dca296e1ed918459a9e
- path:/path/to/repo
- hg+https://...
- tarball+https://example.com/foobar.tar.gz
- sourcehut:~misterio/nix-colors/21c1a380a6915d890d408e9f22203436a35bb2de?host=hg.sr.ht
- file+https://
- file:///some/absolute/file.tar.gz

## An extensive example

```yaml
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  myproject:
    url: github:owner/repo/myproject
    flake: false
imports:
  - ./frontend
  - ./backend
  - myproject
  - myproject/relative/path
```
