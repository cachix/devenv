Most projects out there have a bunch of shell scripts laying around.

Questions arise where to define them and how to provide the tooling to make sure scripts work for all developers.

A simple example defining `silly-example` script:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  packages = [ pkgs.curl pkgs.jq ]; # (1)!

  scripts.silly-example.exec = ''
    curl "https://httpbin.org/get?=$1" | jq '.args'
  '';
}
```

1. See [Packages](packages.md) for an explanation.

Since scripts are exposed when we enter the environment, we can rely that ``packages`` executables are available.

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

(devenv) $ silly-example foo=1
{
  "foo": "1"
}
```

## Pinning packages inside scripts

Sometimes we don't want to expose the tools to the shell but still make sure they are pinned in a script:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  scripts.silly-example.exec = ''
    ${pkgs.curl}/bin/curl "https://httpbin.org/get?=$1" | ${pkgs.jq}/bin/jq '.args'
  '';
}
```

When a package is interpolated in a string, you're referring to it's `$PREFIX` where it was installed.
