[.env](https://github.com/motdotla/dotenv) files were introduced by Heroku in 2012.

If you have a `.env`, you'll see instructions how to enable integration:

```nix title="devenv.nix"
{
  dotenv.enable = true;

  # Optionally, you can choose which filename to load.
  # 
  # dotenv.filename = ".env.production";
  # or
  # dotenv.filename = [ ".env.production" ".env.development" ]
}
```

When the developer environment is loaded, environment variables from `.env` will be loaded
and set into `config.env`.

Variables from `.env` are set using `lib.mkDefault`, meaning that any existing `env` variables set in `devenv.nix` will have priority over them.
