#### Use [`just`](https://just.systems/) in Devenv with Re-usable and Shareable Targets

## Usage

Add the following to your `devenv.nix`:

```nix
{
  just = {
    enable = true; # This enables the command runner
    recipes = {
        convco.enable = true; # This is a built-in recipe.
        # This is a custom recipe.
        hello = {
          enable = true;
          justfile = ''
            # hello from just!
            hello:
              echo Hello World;
          '';
        };
    };
  };
}
```

In addition to the above, we have also integrated `devenv` `scripts` to automatically be added to `just` when required.

For example:

```nix
{
  just.enable = true; # This enables the command runner

  scripts.hello-shell = {
    exec = ''
      echo "Hello Shell!"
    '';
    description = "Hello Shell";
    just.enable = true; # This enables the recipe in just
  };
}
```

This will result in the following:

```
https://devenv.sh (version X.Y.Z): Fast, Declarative, Reproducible, and Composable Developer Environments 💪💪

Run 'just <recipe>' to get started
Available recipes:
    hello-shell                 # Hello Shell
    up                          # Starts processes in foreground. See http://devenv.sh/processes
    version                     # Display devenv version
```

This will add a `shellHook` that generates a `just-flake.just` symlink (which should be gitignore'ed). This will then automatically be used in your `justfile` (assuming you set up your project using `devenv init`).

However, if you have your own `justfile`, you can just add `import 'just-flake.just'` to the top of your `justfile`. See the example below:

```just
# See flake.nix (just-flake)
import 'just-flake.just'

# Display the list of recipes
default:
    @just --list
```

Resulting `devShell` banner and/or output of running `just`:

```
https://devenv.sh (version X.Y.Z): Fast, Declarative, Reproducible, and Composable Developer Environments 💪💪

Run 'just <recipe>' to get started
Available recipes:
    changelog                   # Generate CHANGELOG.md using recent commits
    default                     # Display the list of recipes
    hello                       # hello from just!
    up                          # Starts processes in foreground. See http://devenv.sh/processes
    version                     # Display devenv version
```

---
