Given a hello world example, click on the end of each line to get an explanation:

```nix title="devenv.nix"
{ pkgs, ... }: # (1)!

{ # (2)!
  env.GREET = "hello"; # (3)!

  packages = [ pkgs.jq ];

  enterShell = ''
    echo $GREET
    jq --version
  ''; # (4)!
}
```

1. ``devenv.nix`` is a function with inputs. `pkgs` is an [input](inputs.md) passed as a special argument to the function.
  We use a special input ``...`` at the end as a catch-all to avoid enumerating all of the inputs.
2. Our function is returning an attribute set, similar to an object in JSON.
3. Attributes can be nested and have similar values as in JSON.
4. Values can refer to the inputs. See [Inputs](inputs.md) for how to define inputs.


``enterShell`` allows you to execute bash code once the shell activates, while ``env`` allows you to set environment variables:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

hello
jq-1.6

(devenv) $ echo $GREET
hello
```




See [Nix language tutorial](https://nix.dev/tutorials/first-steps/nix-language) for a 1-2 hour deep dive 
that will allow you to read any Nix file.

!!! note

    We're running [a fundraiser to improve the developer experience around error messages](https://opencollective.com/nix-errors-enhancement), with the goal of lowering the barrier to learning Nix.

## Environment Summary

If you'd like to print the summary of the current environment:

```shell-session
$ devenv info 
...

# env
- DEVENV_DOTFILE: .../myproject/.devenv
- DEVENV_ROOT: .../myproject
- DEVENV_STATE: .../myproject/.devenv/state
- GREET: hello

# packages
- jq-1.6

# scripts

# processes

```
