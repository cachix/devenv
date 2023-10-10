
| Key                          | Value                                                                         |
| ---------------------------- | ----------------------------------------------------------------------------- |
| allowUnfree                  | Allow unfree packages. Defaults to `false`.                                   |
| inputs                       | Defaults to `inputs.nixpkgs.url: github:NixOS/nixpkgs/nixpkgs-unstable`.      |
| inputs.&lt;name&gt;          | Identifier name used when passing the input in your ``devenv.nix`` function.  |
| inputs.&lt;name&gt;.url      | URI specification of the input, see below for possible values.                |
| inputs.&lt;name&gt;.flake    | Does the input contain ``flake.nix`` or ``devenv.nix``. Defaults to ``true``. |
| inputs.&lt;name&gt;.overlays | A list of overlays to include from the input.                                 |
| imports                      | A list of relative paths or references to inputs to import ``devenv.nix``.    |
| permittedInsecurePackages    | A list of insecure permitted packages.                                        |

!!! note "Added in 1.0"

    - relative file support in imports: `./mymodule.nix`

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
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
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
    - Kindly ask them how you can help upgrade, either they will do it or you can do it yourself
    - Make a PR, get the URL of the patch
    - It will take the form of 'github:$GH_USERNAME/nixpkgs/master'
    - Add it to your devenv.yaml like so 

#### This is an example of a rust project that needs a new dependency

```yaml
inputs:
  nixpkgs:
    url: 'github:taylor1791/nixpkgs/master'
  fenix:
    url: 'github:nix-community/fenix'
    inputs:
      nixpkgs:
        follows: nixpkgs
```


