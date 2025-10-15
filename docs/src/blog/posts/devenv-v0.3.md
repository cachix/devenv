---
draft: false 
date: 2022-11-17
authors:
  - domenkozar
---

# devenv 0.3

It has been 3 days since [0.2](devenv-v0.2.md) release, so it's time for 0.3:

# Highlights

- We have a [roadmap](https://devenv.sh/roadmap/)!

- A number of new languages: [OCaml, Closure, PureScript, Lua and CUE](https://devenv.sh/languages/).

- [bobvanderlinden](https://github.com/bobvanderlinden) contributed [Java customization options](https://devenv.sh/reference/options/#languagesjavaenable).

- ``devenv init`` now optionally accepts a directory where to create the structure. Thanks [bobvanderlinden](https://github.com/bobvanderlinden)!

- Installation instructions of devenv have been improved to be more robust.

- [Imports](https://devenv.sh/composing-using-imports/) have been made more robust,
  so that common failure modes now have a reasonable error message.

- ``devenv shell`` now warns when a new version is out (detected via [input](http://devenv.sh/inputs/) updates) and the warning can be [disabled](https://devenv.sh/reference/options/#devenvwarnonnewversion).

# Bug fixes


- [quasigod-io](https://github.com/quasigod-io) made `~/.devenv` respect `XDG_DATA_HOME`.
- [domen](https://github.com/domenkozar) fixed [direnv not reloading](https://github.com/cachix/devenv/issues/10).
- [domen](https://github.com/domenkozar) fixed ``devenv up`` to load the latest shell before starting [processes](https://devenv.sh/processes/).
- [domen](https://github.com/domenkozar) fixed ``devenv init`` not overriding files if they exist.

Domen
