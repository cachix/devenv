## Adding a directory to $PATH

This example adds Elixir install scripts to `~/.mix/escripts`:

```nix
{ ... }:

{
  languages.elixir.enable = true;

  enterShell = ''
    export PATH="$HOME/.mix/escripts:$PATH"
  '';
}
```

## How can I use Rosetta packages?

It's possible to tell Nix to use Intel packages when using macOS ARM:

```nix
{ pkgs, ... }:

let
  rosettaPkgs = 
    if pkgs.stdenv.isDarwin && pkgs.stdenv.isAarch64
    then pkgs.pkgsx86_64Darwin
    else pkgs;
in {
  packages = [
    pkgs.git
    rosettaPkgs.vim
  ];
}
```

## How to exclude packages from the container?

```nix
{ pkgs, ... }: {
  packages = [
    pkgs.git
  ] ++ lib.optionals !config.container.isBuilding [
    pkgs.haskell-language-server
  ];
}
```

# How can I escape Nix curly braces if I'm writing shell code?

```nix
{ pkgs, ... }: {
  scripts.myscript.exec = ''
    foobar=1
    echo ''${foobar}
  '';
}
```

# How to install packages only on Linux or macOS?

```nix
{ pkgs, lib, ... }: {
  packages = [
    pkgs.ncdu
  ] ++ lib.optionals pkgs.stdenv.isLinux [
    pkgs.inotify-tools
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.Security
  ];
}
```
