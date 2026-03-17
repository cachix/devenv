# REPL

`devenv repl` opens an interactive Nix REPL with your devenv environment fully loaded.

## Usage

```shell-session
$ devenv repl
```

This assembles your devenv configuration and drops you into a Nix REPL where you can explore:

- `config` &mdash; the final resolved devenv configuration
- `pkgs` &mdash; the nixpkgs package set
- Any other inputs defined in `devenv.yaml`

## Examples

```nix
nix-repl> config.packages
[ «derivation /nix/store/...» ]

nix-repl> config.languages.python.enable
true

nix-repl> pkgs.hello.version
"2.12.1"
```

The REPL is useful for debugging configuration, inspecting option values, and exploring what is available in your inputs.
