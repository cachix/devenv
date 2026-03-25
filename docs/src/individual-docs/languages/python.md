
# Python

With devenv, you get a ready-to-use Python environment — complete with the version you need, a virtual environment, and your preferred package manager.
This guide walks you through everything from basic setup to advanced topics like native libraries and Nix-packaged dependencies.

## Getting started

To add Python to your project, enable it in your `devenv.nix`:

```nix
{
  languages.python.enable = true;
}
```

This gives you `python3` and [pip](https://pip.pypa.io/) in your shell, using the version of Python that ships with your nixpkgs input.

## Choosing a Python version

To use a specific Python version, set the `version` option:

```nix
{
  languages.python = {
    enable = true;
    version = "3.11";
  };
}
```

You can use a major.minor version like `"3.11"`, or pin an exact release like `"3.11.3"`.

Under the hood, this pulls the Python build from [nixpkgs-python](https://github.com/cachix/nixpkgs-python), which needs to be added as an input in your `devenv.yaml`:

```yaml
inputs:
  nixpkgs-python:
    url: github:cachix/nixpkgs-python
    inputs:
      nixpkgs:
        follows: nixpkgs
```

If you don't need a specific version, you can skip this — devenv uses the default Python 3 from nixpkgs.

## Virtual environments

Python projects typically use [virtual environments](https://docs.python.org/3/library/venv.html) to isolate dependencies.
devenv can create and manage one for you:

```nix
{
  languages.python = {
    enable = true;
    venv.enable = true;
  };
}
```

The virtual environment is stored in `$DEVENV_STATE/venv` and is activated automatically every time you enter the shell.
If the Python interpreter changes (for example, after updating `version`), the virtual environment is recreated.

### Installing packages from requirements

If your project uses a [requirements file](https://pip.pypa.io/en/stable/reference/requirements-file-format/), you can point devenv to it.
Packages are installed with pip when the shell starts:

```nix
{
  languages.python = {
    enable = true;
    venv = {
      enable = true;
      requirements = ./requirements.txt;
      # Or write requirements inline:
      # requirements = ''
      #   requests
      #   flask>=3.0
      # '';
    };
  };
}
```

devenv tracks a checksum of your requirements and your Python interpreter, so packages are only reinstalled when something actually changes.

Set `venv.quiet = true` to suppress pip output during installation.

## Changing the project directory

If your Python code lives in a subdirectory rather than the project root, set the `directory` option:

```nix
{
  languages.python = {
    enable = true;
    directory = "./backend";
    venv.enable = true;
  };
}
```

This changes where devenv looks for files like `pyproject.toml`, `requirements.txt`, and `poetry.lock`.
It also affects where the virtual environment is initialized.
The path can be absolute or relative to the root of the devenv project.

## Package managers

devenv integrates with Python package managers to automatically install dependencies when you enter the shell.

### uv

[uv](https://docs.astral.sh/uv/) is a fast Python package manager that uses `pyproject.toml` to manage dependencies.
devenv can run [`uv sync`](https://docs.astral.sh/uv/reference/cli/#uv-sync) automatically when you enter the shell:

```nix
{
  languages.python = {
    enable = true;
    venv.enable = true;
    uv = {
      enable = true;
      sync.enable = true;
    };
  };
}
```

This expects a `pyproject.toml` in your project directory (or the directory set by `languages.python.directory`).
devenv ensures uv uses the Python interpreter from your configuration rather than downloading its own.

#### Dependency groups and extras

You can control exactly which dependency groups and extras uv installs:

```nix
{
  languages.python = {
    enable = true;
    venv.enable = true;
    uv = {
      enable = true;
      sync = {
        enable = true;
        allGroups = true;             # Install all dependency groups
        # groups = [ "dev" "test" ];  # Or pick specific ones
        # extras = [ "plotting" ];    # Specific extras
        # allExtras = true;           # All extras
      };
    };
  };
}
```

For uv workspaces and additional sync flags, see the options reference below.

### Poetry

[Poetry](https://python-poetry.org/) manages Python project dependencies and packaging.
devenv can run [`poetry install`](https://python-poetry.org/docs/cli/#install) and activate the virtual environment for you:

```nix
{
  languages.python = {
    enable = true;
    poetry = {
      enable = true;
      install.enable = true;
      activate.enable = true;
    };
  };
}
```

This expects `pyproject.toml` and `poetry.lock` in your project directory.
Poetry creates its virtual environment in `.venv` inside your project directory (rather than `$DEVENV_STATE/venv`).

#### Install options

You can fine-tune what Poetry installs:

```nix
{
  languages.python = {
    enable = true;
    poetry = {
      enable = true;
      install = {
        enable = true;
        installRootPackage = true;    # Install your project itself
        groups = [ "dev" "test" ];    # Specific dependency groups
        # ignoredGroups = [ "docs" ]; # Groups to skip
        # allExtras = true;           # All extras
        quiet = true;                 # Suppress output
      };
      activate.enable = true;
    };
  };
}
```

See the options reference below for the full set of install options.

## Using Nix Python packages

Nix can build and cache Python packages, so you don't need to compile them from source or download them from [PyPI](https://pypi.org/).
This is especially useful for packages that are difficult to install with pip — like `tkinter`, or packages with complex native dependencies.

Use `withPackages` to bundle Nix-built packages into your Python interpreter:

```nix
{
  languages.python = {
    enable = true;
    package = pkgs.python3.withPackages (ps: [
      ps.numpy
      ps.tkinter
    ]);
    venv.enable = true;
  };
}
```

You can combine this with uv or Poetry — the Nix packages provide a base, and your package manager handles the rest.
When a virtual environment is active, packages installed by pip/uv/Poetry take priority over the Nix-provided ones.

## Native libraries

Some Python packages — like [Pillow](https://pillow.readthedocs.io/), [grpcio](https://grpc.io/docs/languages/python/), or [transformers](https://huggingface.co/docs/transformers/) — link against native C/C++ libraries at build time.
In a Nix-based environment, these libraries aren't in the usual system locations, so pip may fail to find them.

To fix this, add the native libraries to `packages`:

```nix
{
  packages = [ pkgs.cairo pkgs.zlib ];

  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      pillow
      grpcio-tools
    '';
  };
}
```

devenv adds these libraries to `LD_LIBRARY_PATH` (Linux) or `DYLD_LIBRARY_PATH` (macOS), and includes the C runtime library automatically.

For more control over library search paths, use the `libraries` option.

### Manylinux support

On Linux, some pre-built Python wheels depend on [manylinux](https://github.com/pypa/manylinux) compatibility libraries.
If you run into related installation errors, enable manylinux support:

```nix
{
  languages.python.manylinux.enable = true;
}
```

## Building Python projects with Nix

You can turn your Python project into a reproducible Nix build using [uv2nix](https://github.com/pyproject-nix/uv2nix).
This is useful for creating deployable artifacts or integrating with other Nix-based infrastructure:

```nix
{ config, ... }:

let
  myapp = config.languages.python.import ./path/to/project {};
in
{
  languages.python.enable = true;
  packages = [ myapp ];
  outputs = { inherit myapp; };
}
```

The project directory must contain a `pyproject.toml`.
The package name is inferred automatically, or you can set it explicitly with `{ packageName = "my-custom-name"; }`.

## Language server

devenv includes [Pyright](https://github.com/microsoft/pyright) as the default Python language server, enabled automatically.

To use a different language server, or disable it entirely:

```nix
{
  languages.python = {
    enable = true;
    # Use a different LSP:
    lsp.package = pkgs.python3Packages.python-lsp-server;
    # Or disable it entirely:
    # lsp.enable = false;
  };
}
```

## Putting it all together

Here's a complete example of a Django project using Poetry, with a PostgreSQL database and a dev server managed by devenv:

```nix
{ config, pkgs, ... }:

let
  db_user = "postgres";
  db_name = "myapp";
in
{
  languages.python = {
    enable = true;
    version = "3.11";
    poetry = {
      enable = true;
      install.enable = true;
      activate.enable = true;
    };
  };

  env = {
    DATABASE_URL = "postgres://${db_user}@/${db_name}?host=${config.env.PGHOST}";
    SECRET_KEY = "dev-only-secret"; # Do not use in production
  };

  services.postgres = {
    enable = true;
    initialDatabases = [{ name = db_name; user = db_user; }];
    # Django's test runner needs CREATEDB to create a test database
    initialScript = "ALTER ROLE ${db_user} CREATEDB;";
  };

  processes.runserver = {
    exec = "exec python manage.py runserver";
    after = [ "devenv:processes:postgres" ];
  };
}
```

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
