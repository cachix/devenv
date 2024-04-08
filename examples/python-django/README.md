# `devenv-django`

This example demonstrates using [devenv.sh](https://devenv.sh/) alongside [`poetry`](https://python-poetry.org/docs/) for building a `Django` development environment.

Specifically,  `devenv` uses `nix` to install system level packages like `postgresql_14` from `nixpkgs` & `poetry` uses `pip` to install `Python` packages from `pypi`

> **Note**:  Also see https://github.com/nix-community/poetry2nix/ which converts `poetry` projects to `nix`,  this example uses both tools separately


---


## Installation

Install [`devenv.sh`](https://devenv.sh/getting-started)


---


## Usage

`devenv` enables defining scripts in `devenv.nix` that are automatically added to the shell path ...

- Launch a development server via `devenv up`

- Launch a development shell via `devenv shell`

> [!WARNING]  
> This example depends on `Postgres`,  so please ensure that it's running via `devenv up` (or automatically via `direnv`) before running `python manage.py ...`

- Run tests via `devenv test` 

> [!TIP]  
> See [`devenv.sh`](https://devenv.sh/tests/) for more information)
