# REPL

`devenv repl` opens an interactive Nix REPL with your devenv environment fully loaded.

## Usage

```shell-session
$ devenv repl
```

This assembles your devenv configuration and drops you into a Nix REPL where you can explore:

- `devenv` &mdash; the evaluated devenv attribute set:
    - `devenv.config` &mdash; the final resolved configuration (options like `languages`, `packages`, `services`).
    - `devenv.options` / `devenv.optionsJSON` &mdash; option definitions and their JSON-serialized form.
    - `devenv.shell` &mdash; the dev shell derivation.
    - `devenv.build` &mdash; declared `outputs.*` derivations.
- `pkgs` &mdash; the nixpkgs package set as configured by your project (same as `devenv.pkgs`).

## Examples

```nix
nix-repl> devenv.config.packages
[ «derivation /nix/store/...» ]

nix-repl> devenv.config.languages.python.enable
true

nix-repl> pkgs.hello.version
"2.12.1"
```

The REPL is useful for debugging configuration, inspecting option values, and exploring what is available in your inputs.
