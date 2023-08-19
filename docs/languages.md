# Languages

What if you could have the tooling for any programming language by flipping a toggle?

```nix title="devenv.nix"
{ pkgs, ... }:

{
  languages.python.enable = true;
  languages.python.version = "3.11.3";

  languages.rust.enable = true;
  # https://devenv.sh/reference/options/#languagesrustchannel
  languages.rust.channel = "stable";
}
```

``devenv`` will provide executables for both languages:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

(devenv) $ python --version
Python 3.11.3
```

## Supported languages

{%
  include-markdown "languages-all.md"
%}

You can find all supported options for languages [here](https://devenv.sh/reference/options/#languagesansibleenable).
