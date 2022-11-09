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

To search for available packages, use [package search](https://search.nixos.org/packages?channel=unstable)
provided by Nix community.

You need to refer to the **unique package name, highlighted as a link** in the search results. Sometimes also called the attribute name. 

For example, if you [search for ``ncdu``](https://search.nixos.org/packages?channel=unstable&query=ncdu), you'll find ``ncdu`` and ``ncdu_1`` unique names (besides a few R packages).

!!! note

    If you would find ``devenv search`` command useful, vote for it [here](https://github.com/cachix/devenv/issues/4).