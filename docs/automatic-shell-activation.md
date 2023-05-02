You can configure ``devenv`` to **seamlessly switch development environments** when navigating between project directories.

This feature relies on a separate tool called [direnv](https://direnv.net) (not to be confused with devenv).

## Installing ``direnv``

1. [Install direnv](https://direnv.net/docs/installation.html#from-system-packages)
2. [Add the direnv hook to your shell](https://direnv.net/docs/hook.html)

## Using ``direnv``

Once installed, you'll see a warning in your shell the next time you enter the project directory:

```
direnv: error ~/myproject/.envrc is blocked. Run `direnv allow` to approve its content
```

Run ``direnv allow`` to enable the environment. It will now be automatically loaded and unloaded whenever you enter and exit the project directory.

```shell-session
$ cd /home/user/myproject/
direnv: loading ~/myproject/.envrc
Building shell ...
Entering shell ...

(devenv) $
```

## Customizing PS1

If you'd like to use direnv and have your prompt be aware of it,
we recommend [installing Starship](https://starship.rs/guide/).

## Managing the `.direnv` directory

The `.direnv` directory will be added to your `.gitignore` file by default when you run `devenv init`.
