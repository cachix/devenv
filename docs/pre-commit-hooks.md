``devenv`` has first-class integration for [pre-commit](https://pre-commit.com/) via [pre-commit-hooks.nix](https://github.com/cachix/pre-commit-hooks.nix).

We recommend a two-step approach for integrating your linters and formatters.

## 1) Make sure that commits are well-formatted at commit time

```nix title="devenv.nix"
{ inputs, ... }:

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

If you commit a Python or Markdown file or a script, these hooks will run at commit time.

## 2) Verify formatting in CI

Run ``devenv ci``.

See [the list of all available hooks](reference/options.md#pre-commithooks).
