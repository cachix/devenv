---
draft: false 
date: 2022-11-27
authors:
  - domenkozar
---

# devenv 0.4

# Highlights

- New command ``devenv info`` shows locked inputs, environment variables, scripts, processes
  and packages exposed in the environment.

- Tracebacks [are now printed with most relevent information at the bottom](https://github.com/NixOS/nix/pull/7334).

- New option `process.implementation` allows you to choose how processes are run. New supported options are [overmind](https://github.com/DarthSim/overmind) and [process-compose](https://github.com/F1bonacc1/process-compose).

- Instead of passing each input separately in 
  `devenv.nix`, the new prefered and documented way is via `inputs` argument, for example `inputs.pre-commit-hooks`.

- [samjwillis97](https://github.com/samjwillis97) contributed support for MongoDB.

- [shyim](https://github.com/shyim) contributed MySQL/MariaDB support.

- [shyim](https://github.com/shyim) made PHP configuration more configurable, for example you can now set extensions.

- [JanLikar](https://github.com/JanLikar) improved PostgreSQL support to expose `psql-devenv` script for
  connecting to the cluster.

- [datakurre](https://github.com/datakurre) added [robotframework](https://robotframework.org/) support.


# Bug fixes

- Composing using inputs has been fixed.

- It's now possible to use `devenv` on directories with spaces.

- Update checker is no longer using environment variables to avoid some corner cases.