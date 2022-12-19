To get started using [diffstatic - a structural diff that understands syntax for over 30 langauges](https://difftastic.wilfred.me.uk/), flip a toogle:


```nix title="devenv.nix"
{ pkgs, ... }:

{
    diffstatic.enable = true;
}
```

Once you run `devenv shell`, you should see the following output when using `git diff`:


![Screenshot of difftastic and JS](https://github.com/Wilfred/difftastic/raw/master/img/js.png)