---
draft: false 
date: 2022-11-14
authors:
  - domenkozar
---

# devenv 0.2

After an intense weekend and lots of incoming contributions, `v0.2` is out!

# Highlights

- All the ``devenv.nix`` options you can define now come as an input (instead of being packaged with each devenv release). To update the options you can run ``devenv update`` and it will match [devenv.nix reference](reference.md#options).

- New ``devenv search`` command:

```shell-session
$ devenv search ncdu
name         version  description
pkgs.ncdu    2.1.2    Disk usage analyzer with an ncurses interface
pkgs.ncdu_1  1.17     Disk usage analyzer with an ncurses interface
pkgs.ncdu_2  2.1.2    Disk usage analyzer with an ncurses interface

Found 3 results.
```

- [shyim](https://github.com/shyim) contributed Redis support and is working on MySQL.

- Languages: [raymens](https://github.com/raymens) contributed dotnet, [ankhers](https://github.com/ankhers) contributed Elixir and Erlang support.

- If ``devenv.local.nix`` exists it's now also loaded, allowing you to override git committed ``devenv.nix`` with local changes. Hurrah composability!

# Bug fixes


- Variables like ``env.DEVENV_ROOT``, ``env.DEVENV_STATE`` and ``env.DEVENV_DOTFILE`` are now absolute paths paths
- [shyim](https://github.com/shyim) fixed ``/dev/stderr`` that is in some environments not available.
- [domen](https://github.com/domenkozar) fix shell exiting on non-zero exit status code. 

Domen