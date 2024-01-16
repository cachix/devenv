To get started using [Delta, a syntax-highlighting pager for git, diff, and grep output](https://dandavison.github.io/delta/), flip a toggle:

```nix title="devenv.nix"
{ pkgs, ... }:

{
    delta.enable = true;
}
```
