``devenv`` has first-class integration of [pre-commit](https://pre-commit.com/) via [pre-commit-hooks.nix](https://github.com/cachix/pre-commit-hooks.nix).

To integrate your linters and formatters, we recommend two step approach.

## 1) Before commit time, to make sure commits are well formatted

```nix title="devenv.nix"
{ pkgs, ... }:

{
  pre-commit.hooks = {
    # lint shell scripts
    shellcheck.enable = true;
    # execute example shell from Markdown files
    mdsh.enable = true;
    # format Python code
    black.enable = true;
  };
}
```

In action:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

pre-commit installed at .git/hooks/pre-commit
```

If you commit a Python file, markdown file or a script, these hooks will run at commit time.

## 2) Once on a CI, to make sure formatting/linting step is guaranted 

Run ``devenv ci``.

See [the list of all available hooks](reference/options.md#pre-commithooks).