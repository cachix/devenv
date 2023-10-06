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

## Searching for a file

If you'd like to see what package includes a specific file, for example `libquadmath.so`:

```shell-session
$ nix run github:mic92/nix-index-database libquadmath.so
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
