Most projects out there have a bunch of shell scripts lying around.

Questions arise as to where to define scripts and how to provide the tooling to make sure they work for all developers.

A simple example defining `silly-example` script:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  packages = [ pkgs.curl pkgs.jq ]; # (1)!

  scripts.silly-example.exec = ''
    curl "https://httpbin.org/get?$1" | jq '.args'
  '';
}
```

1. See [Packages](packages.md) for an explanation.

Since scripts are exposed when we enter the environment, we can rely on ``packages`` executables being available.

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
    ${pkgs.curl}/bin/curl "https://httpbin.org/get?$1" | ${pkgs.jq}/bin/jq '.args'
  '';
}
```

When a package is interpolated in a string, you're referring to the path where it is located.

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

(devenv) $ silly-example foo=1
{
  "foo": "1"
}
```
