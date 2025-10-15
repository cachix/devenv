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

### Aliases & args
Here's an example that shows how to define an alias & forward arguments:
```
scripts.foo.exec = ''
  npx @foo/cli "$@";
'';
```

## Runtime packages

Sometimes you need packages available only when a specific script runs, without adding them to the global environment. You can specify runtime packages using the `packages` attribute:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  scripts.analyze-json = {
    exec = ''
      # Both curl and jq are available when this script runs
      curl "https://httpbin.org/get?$1" | jq '.args'
    '';
    packages = [ pkgs.curl pkgs.jq ];
    description = "Fetch and analyze JSON";
  };
}
```

The `packages` attribute ensures these tools are available in the script's PATH without polluting the global development environment.

## Pinning packages inside scripts

Alternatively, you can directly reference package paths in your script:

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

## Using your favourite language

Scripts can also execute using a package and have a description, which can be useful in your `enterShell`.

!!! tip "Consider using tasks for shell initialization"
    For operations that need to run when entering the shell, consider using [tasks with the `before` attribute](tasks.md#entershell-entertest) instead of `enterShell`. Tasks provide better control over execution order and dependencies.

```nix title="devenv.nix"
{ pkgs, config, lib, ... }:

{
  scripts.python-hello = {
    exec = ''
      print("Hello, world!")
    '';
    package = config.languages.python.package;
    description = "hello world in Python";
  };

  scripts.nushell-greet = {
    exec = ''
      def greet [name] {
        ["hello" $name]
      }
      greet "world"
    '';
    package = pkgs.nushell;
    binary = "nu";
    description = "Greet in Nu Shell";
  };

  scripts.file-example = {
    exec = ./file-script.sh;
    description = "Script loaded from external file";
  };

  enterShell = ''
    echo
    echo 🦾 Helper scripts you can run to make your development richer:
    echo 🦾
    ${pkgs.gnused}/bin/sed -e 's| |••|g' -e 's|=| |' <<EOF | ${pkgs.util-linuxMinimal}/bin/column -t | ${pkgs.gnused}/bin/sed -e 's|^|🦾 |' -e 's|••| |g'
    ${lib.generators.toKeyValue {} (lib.mapAttrs (name: value: value.description) config.scripts)}
    EOF
    echo
  '';
}
```

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

🦾 Helper scripts you can run to make your development richer:
🦾
🦾 python-hello     Hello world in Python
🦾 nushell-greet    Greet in Nu Shell
🦾 file-example     Script loaded from external file

(devenv) $
```
