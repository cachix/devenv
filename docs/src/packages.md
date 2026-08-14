# Packages

Packages allow you to add executables and libraries/headers to your environment.

To declare packages, refer to the `pkgs` input and specify `packages` as a list:
Add packages to the `packages` list

```nix title="devenv.nix"
{ pkgs, ... }:

{
  packages = [
    # Executables
    pkgs.git
    pkgs.jq
    # Libraries
    pkgs.libffi
    pkgs.zlib
  ];
}
```

Packages are added to the PATH when you activate the shell:

```shell-session
$ jq
jq: command not found

$ devenv shell
Building shell ...
Entering shell ...

(devenv) $ jq --version
jq-1.6
```

## Installing a specific version

Add the [nixpkgs-multiverse](https://github.com/fzakaria/nixpkgs-multiverse) input once:

```shell-session
$ devenv inputs add nixpkgs-multiverse github:fzakaria/nixpkgs-multiverse
```

Packages are then available by their Nixpkgs attribute and version:

```nix title="devenv.nix"
{ pkgs, multiverse, ... }:

{
  packages = [
    pkgs.git
    multiverse.cmake."3.16.5"
    multiverse.bun."0.7.0"
  ];
}
```

The `nixpkgs-multiverse` input is pinned in `devenv.lock`. Historical Nixpkgs revisions are fetched lazily, so only
revisions providing packages you use are downloaded. The unmodified input remains available as
`inputs."nixpkgs-multiverse"` for advanced use. See [Pinning an individual package version](pinning.md#pinning-an-individual-package-version)
for the reproducibility and update workflow.

Multiverse indexes top-level package attributes from published Nixpkgs releases and `nixos-unstable` channel
revisions. If a package or version is not indexed, you can still
[fetch it from another nixpkgs input](recipes/nix.md).

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

This will search [available packages](https://search.nixos.org/packages?channel=unstable&query=ncdu) for the exact pinned version of Nixpkgs input in your ``devenv.lock``.

## Searching for a file

If you'd like to see what package includes a specific file, for example `libquadmath.so`:

```shell-session
$ nix run github:nix-community/nix-index-database libquadmath.so
(rPackages.RcppEigen.out)                       302,984 x /nix/store/24r9jkqyf2nd5dlg1jyihfl82sa9nwwb-gfortran-12.3.0-lib/lib/libquadmath.so.0.0.0
(zsnes2.out)                                    693,200 x /nix/store/z23qmfjaj5p50n3iki7zkjjgjzia16v1-gcc-12.3.0-lib/lib/libquadmath.so.0.0.0
(zulip.out)                                           0 s /nix/store/xnlcrrg3b9fgwry6qh3fxk3hnb0whs5z-zulip-5.10.2-usr-target/lib/libquadmath.so.0.0.0
(zulip.out)                                           0 s /nix/store/xnlcrrg3b9fgwry6qh3fxk3hnb0whs5z-zulip-5.10.2-usr-target/lib64/libquadmath.so.0.0.0
(zulip.out)                                           0 s /nix/store/48dnfgadck1mzncy002cs1a9hpddmdmz-zulip-5.10.2-fhs/usr/lib/libquadmath.so.0.0.0
(zettlr-beta.out)                                     0 s /nix/store/nlq9rpakv852kkm7lwhzgb8iap1izpdm-zettlr-beta-3.0.0-beta.7-fhs/usr/lib/libquadmath.so.0.0.0
(zettlr-beta.out)                                     0 s /nix/store/8ypzmv66kvi6qrdlga9yg60gl396n7ny-zettlr-beta-3.0.0-beta.7-usr-target/lib/libquadmath.so.0.0.0
(zettlr-beta.out)                                     0 s /nix/store/8ypzmv66kvi6qrdlga9yg60gl396n7ny-zettlr-beta-3.0.0-beta.7-usr-target/lib64/libquadmath.so.0.0.0
(zettlr.out)                                          0 s /nix/store/5xq9qch1fnknn3z97wcdvcf5vgjfm2ip-zettlr-2.3.0-fhs/usr/lib/libquadmath.so.0.0.0
(zecwallet-lite.out)                                  0 s /nix/store/rllm8zagppnjf4kh14drwwg93gsxwaja-zecwallet-lite-1.8.8-fhs/usr/lib/libquadmath.so.0.0.0
...
```
