You can configure ``devenv`` to **seamlessly switch development environments** when navigating between project directories.

This feature relies on a separate tool called [direnv](https://direnv.net) (not to be confused with devenv).

## Setup

1. [Install direnv](https://direnv.net/docs/installation.html#from-system-packages)
2. [Add the direnv hook to your shell](https://direnv.net/docs/hook.html)

## Use

Once installed, you'll see a warning in your shell the next time you enter the project directory:

```
direnv: error ~/myproject/.envrc is blocked. Run `direnv allow` to approve its content
```

Run ``direnv allow`` to enable the environment. It'll now be automatically loaded and unloaded whenever you enter and exit the project directory.

```shell-session
$ cd /home/user/myproject/
direnv: loading ~/myproject/.envrc
Building shell ...
Entering shell ...

(devenv) $
```