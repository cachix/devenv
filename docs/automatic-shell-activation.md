You can configure ``devenv`` to **seamlessly switch development environments** when navigating between project directories.

This feature relies on a separate tool called [direnv](https://direnv.net) (not to be confused with devenv).

## Installing ``direnv``

1. [Install direnv](https://direnv.net/docs/installation.html#from-system-packages)
2. [Add the direnv hook to your shell](https://direnv.net/docs/hook.html)

## Configure shell activation

To enable automatic shell activation, create a `.envrc` file in your project directory with the following content:

```bash
eval "$(devenv direnvrc)"

use devenv
```

This file configures direnv to use devenv for shell activation.

`devenv init` will create this file by default when you initialize a new project.

## Approving and loading the shell

Once the `.envrc` file is in place, you'll see a warning in your shell:

```
direnv: error ~/myproject/.envrc is blocked. Run `direnv allow` to approve its content
```

Run `direnv allow` to approve the `.envrc` file. This step is a security measure to ensure you've reviewed the content before allowing it to modify your shell environment.

After approval, direnv will automatically load and unload the devenv environment whenever you enter and exit the project directory:

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

## Ignoring the `.direnv` directory

The `.direnv` directory will be added to your `.gitignore` file by default when you run `devenv init`.

If you need to add it manually, run:

```
echo ".direnv" >> .gitignore
```
