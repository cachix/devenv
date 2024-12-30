You can configure ``devenv`` to **seamlessly switch development environments** when navigating between project directories.

This feature relies on a separate tool called [direnv](https://direnv.net) (not to be confused with devenv).

## Installing ``direnv``

1. [Install direnv](https://direnv.net/docs/installation.html#from-system-packages)
2. [Add the direnv hook to your shell](https://direnv.net/docs/hook.html)

## Configure shell activation

To enable automatic shell activation, create an `.envrc` file in your project directory with the following content:

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

To add it manually, run:

```shell-session
echo ".direnv" >> .gitignore
```

## Manually managing updates to direnvrc

We occasionally make updates to our direnv integration script, also known as the `direnvrc`.
Devenv will use the latest compatible version if set up using the method described above in [Configure Shell Activation](#configure-shell-activation).

Alternatively, you can pin the `direnvrc` to a specific version from the source repository.
This approach allows you audit the `direnvrc` script and have full control over when it is updated.
The downside is that you will have to manually update the URL and content hash of the script for every single project individually.

We strongly recommend using the approach that supports automated upgrades described in [Configure Shell Activation](#configure-shell-activation).

The `direnvrc` can be found at:

```text
https://raw.githubusercontent.com/cachix/devenv/VERSION/direnvrc
```

Replace `VERSION` with a valid git tag or branch name.

To use it in your `.envrc`, first compute its sha256 hash:

```shell-session
direnv fetchurl "https://raw.githubusercontent.com/cachix/devenv/VERSION/direnvrc"
```
```shell-session
Found hash: <HASH>
```

Then modify your `.envrc`, updating the URL and inserting the computed hash from the previous step:

```bash
source_url "https://raw.githubusercontent.com/cachix/devenv/VERSION/direnvrc" "<HASH>"

use devenv
```
