## Adding a directory to $PATH

For example adding Elixir install scripts into `~/.mix/escripts`

```nix
{ ... }:

{
  languages.elixir.enable = true;

  enterShell = ''
    export PATH="$HOME/.mix/escripts:$PATH"
  '';
}
```
