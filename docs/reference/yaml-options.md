
| Key                          | Value                                                                         |
| ---------------------------- | ----------------------------------------------------------------------------- |
| allowUnfree                  | Allow unfree packages. Defaults to `false`.                                   |
| allowBroken                  | Allow packages marked as broken. Defaults to `false`.                         |
| inputs                       | Defaults to `inputs.nixpkgs.url: github:NixOS/nixpkgs/nixpkgs-unstable`.      |
| inputs.&lt;name&gt;          | Identifier name used when passing the input in your ``devenv.nix`` function.  |
| inputs.&lt;name&gt;.url      | URI specification of the input, see below for possible values.                |
| inputs.&lt;name&gt;.flake    | Does the input contain ``flake.nix`` or ``devenv.nix``. Defaults to ``true``. |
| inputs.&lt;name&gt;.overlays | A list of overlays to include from the input.                                 |
| imports                      | A list of relative paths or references to inputs to import ``devenv.nix``.    |
| permittedInsecurePackages    | A list of insecure permitted packages.                                        |
| clean.enabled                | Clean the environment when entering the shell. Defaults to `false`.           |
| clean.keep                   | A list of environment variables to keep when cleaning the environment.        |
| impure                       | Relax the hermeticity of the environment.                                     |

!!! note "Added in 1.0"

    - relative file support in imports: `./mymodule.nix`
    - `clean`
    - `impure`
    - `allowBroken`

## inputs.&lt;name&gt;.url

- github:NixOS/nixpkgs/master
- github:NixOS/nixpkgs?rev=238b18d7b2c8239f676358634bfb32693d3706f3
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
allowUnfree: true
allowBroken: true
clean:
  enabled: true
  keep:
    - EDITOR
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  myproject:
    url: github:owner/myproject
    flake: false
  myproject2:
    url: github:owner/myproject
    overlays:
      - default
imports:
  - ./frontend
  - ./backend
  - ./mymodule.nix
  - myproject
  - myproject/relative/path
```

!!! note "Added in 1.0"

    - relative file support in imports: `./mymodule.nix`

### What if a package is out of date?

- Open [nixpkgs repo](https://github.com/NixOS/nixpkgs) and press `t` to search for your package.
- Try to update/change the package using [the nixpkgs contributing guide](https://nixos.org/manual/nixpkgs/stable/#chap-quick-start), optionally contacting the maintainer for help if you get stuck.
- Make a PR and remember the branch name.
- Add it to your devenv.yaml using the nixpkgs input in form of 'github:$GH_USERNAME/nixpkgs/master', edit `devenv.yaml`:

```yaml
inputs:
  nixpkgs:
    url: 'github:$GH_USERNAME/nixpkgs/MYBRANCH'
```


