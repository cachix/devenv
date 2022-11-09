To make **switching between dev environments** seamless,
a project called [direnv](https://direnv.net) (not to be confused with devenv)
is used to **activate your environment when you enter the directory** of your project.

## Setup

1. [Install the executable](https://direnv.net/docs/installation.html#from-system-packages)
2. [Insert the hook into your shell](https://direnv.net/docs/hook.html)

## Use

If you have installed it successfully, next time your enter project you should see a warning:

```
direnv: error ~/myproject/.envrc is blocked. Run `direnv allow` to approve its content
```

Once you run ``direnv allow``, it will automatically enter the environment once you change the directory:

```shell-session
$ cd /home/user/myproject/
direnv: loading ~/myproject/.envrc
Building shell ...
Entering shell ...

(devenv) $
```