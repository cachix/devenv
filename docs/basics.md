Given a hello world example, click on the end of each line to get an explanation:

```nix title="devenv.nix"
{ pkgs, ... }: # (1)!

{ # (2)!
  env.UNICORNS = "yes"; # (3)!

  enterShell = ''
    echo hello
    ${pkgs.jq}/bin/jq --version
  ''; # (4)!
}
```

1. ``devenv.nix`` is a function with inputs. We use a special input ``...`` at the end as a catch-all to avoid enumuerating all of the inputs.
2. Our function is returning an attribute set, similar to an object in JSON.
3. Attributes can be nested and have a similar values as in JSON.
4. Values can refer to the inputs. See [Imports & Inputs](imports-and-inputs.md) how to define inputs.


``enterShell`` allows you to execute bash code once the shell activates, while ``env`` allows you to set environment variables:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

hello
jq-1.6

(devenv) $ echo $UNICORNS
yes
```




See [Nix language tutorial](https://nix.dev/tutorials/nix-language) for a 1-2 hours deep dive 
that will allow you to read any Nix file.

!!! note

    We're running [a fundraising to improve developer experience around errors messages](https://opencollective.com/nix-errors-enhancement), which enables us to lower the barrier to learning Nix.