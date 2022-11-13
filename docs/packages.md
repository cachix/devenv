# Packages

Packages allow you to expose executables and libraries/headers in your environment.

To declare packages refer to the `pkgs` input and specifying `packages` as a list:

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

If you activate your enviroment you should have tools available:
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

To search for available packages, use ``devenv search NAME``:

```shell-session
$ devenv search ncdu
name         version  description
pkgs.ncdu    2.1.2    Disk usage analyzer with an ncurses interface
pkgs.ncdu_1  1.17     Disk usage analyzer with an ncurses interface
pkgs.ncdu_2  2.1.2    Disk usage analyzer with an ncurses interface

Found 3 results.
```

This will search [available packages](https://search.nixos.org/packages?channel=unstable&query=ncdu),
for the exact pinned version of nixpkgs input in your ``devenv.lock``.
