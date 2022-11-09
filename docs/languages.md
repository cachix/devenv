What if you could have the tooling for any programming language by flipping a toggle?

```nix title="devenv.nix"
{ pkgs, ... }:

{
  languages.python.enable = true;
  languages.typescript.enable = true;
}
```

``devenv`` will provide executables for both languages:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

Python 3.10.8
tsc --version
Version 4.8.4
(devenv) $ 
```

See [all-languages](https://github.com/cachix/devenv/blob/main/examples/all-languages/devenv.nix) example to see a list of currently supported languages.