To get started using [Codespaces](https://github.com/features/codespaces),
you flip a toogle:


```nix title="devenv.nix"
{ pkgs, ... }:

{
    devcontainer.enable = true;
}
```

Once you run ``devenv shell``, you should see auto-generated `.devcontainer.json`.


If you commit that file to the git repository and push it, you're good to go.