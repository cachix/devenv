# Packages

Packages allow you to expose executables and libraries/headers in your environment.

To declare packages, refer to the `pkgs` input and specify `packages` as a list:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  packages = [ 
    pkgs.git 
    pkgs.jq
    pkgs.libffi
    pkgs.zlib
  ];
}
```

If you activate your enviroment, you should have tools available:
```shell-session
$ jq
jq: command not found

$ devenv shell
Building shell ...
Entering shell ...

(devenv) $ jq --version
jq-1.6
```

## Searching

To search for available packages, use ``devenv search <NAME>``:

```shell-session
$ devenv search ncdu
name         version  description
----         -------  -----------
pkgs.ncdu    2.2.1    Disk usage analyzer with an ncurses interface
pkgs.ncdu_1  1.17     Disk usage analyzer with an ncurses interface
pkgs.ncdu_2  2.2.1    Disk usage analyzer with an ncurses interface


No options found for 'ncdu'.

Found 3 packages and 0 options for 'ncdu'.
```

This will search [available packages](https://search.nixos.org/packages?channel=unstable&query=ncdu)
for the exact pinned version of Nixpkgs input in your ``devenv.lock``.
