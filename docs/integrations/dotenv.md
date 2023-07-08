[.env](https://github.com/motdotla/dotenv) files were introduced by Heroku in 2012.

If you have a `.env`, you'll see instructions how to enable integration:

```nix title="devenv.nix"
{
  dotenv.enable = true;
}
```

When the developer environment is loaded, environment variables from `.env` will be loaded
and set into `config.env`.

Variables from `.env` are set using `lib.mkDefault`, meaning that any existing `env` variables set in `devenv.nix` will have priority over them.

!!! note

    This feature won't work when [using with Flakes](../guides/using-with-flakes.md)
    due to [design decisions](https://github.com/NixOS/nix/issues/7107).