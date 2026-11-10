# Environment composition prototype

This is an executable design experiment related to [#3348](https://github.com/cachix/devenv/pull/3348)
and [#2761](https://github.com/cachix/devenv/issues/2761).
The helpers are **not exposed in devenv's public module API**. Existing environments are unchanged.

## Proposed behavior

Environment values have two independent phases:

1. Initial values are available before stdenv setup starts. Hooks can read or modify them.
2. Explicit final operations run after stdenv, including registered `postHooks` and the global `userHook`.
   The resulting values are captured by Nix, so JSON and shell activation agree.

The prototype library is `src/modules/lib/env.nix`. A prospective configuration might use:

```nix
env.CC = envHelpers.set "zig cc";
env.PKG_CONFIG_PATH = envHelpers.prepend ":" [ "/custom/lib/pkgconfig" ];
env.NIX_CFLAGS_COMPILE = envHelpers.append " " [ "-DCUSTOM" ];
env.dontAddPythonPath = envHelpers.beforeSetup "1";
env.CUSTOM_TOOL = envHelpers.default "my-tool";
```

`envHelpers` is the imported prototype library, not an existing module argument.
Plain values retain today's behavior as setup inputs. They are not reapplied afterward.

Use `combine` when one module needs to supply both phases or multiple operations:

```nix
env.EXAMPLE = envHelpers.combine [
  (envHelpers.beforeSetup "initial")
  (envHelpers.set "final-base")
  (envHelpers.prepend ":" [ "prefix" ])
  (envHelpers.append ":" [ "suffix" ])
];
```

Setup sees `initial`; the captured value is `prefix:final-base:suffix`.

## Module merge rules

- `set` selects the final base value. Without it, the completed stdenv value is the base.
- `default` supplies a base only if the variable is unset. An explicitly empty value stays empty.
- Prepends and appends are collected in Nix module definition order, respecting `mkBefore` and `mkAfter`.
  All prepends precede the base; all appends follow it. A later `set` does not erase extension contributions.
- Extensions for one variable must use the same explicit separator. An empty separator concatenates directly.
  An empty or unset base does not introduce an extra separator. Empty contribution lists are no-ops.
- Different `set` values, different `default` values, or incompatible separators fail during evaluation.
  Identical replacements are accepted.
- Ordinary Nix override priorities apply to the whole variable. `mkForce` therefore discards lower-priority operations.
- Initial definitions use the existing `types.anything` merge rules, including conflict errors.
- Lists, booleans, paths, packages and strings use unstructured derivation string conversion for final values.
  Initial `null` values remain omitted. Final `set null` and `default null` are rejected as ambiguous.
- Helper targets must be shell identifiers outside the reserved `_devenv_env_` namespace.

## Running the experiment

From the repository development shell:

```sh
devenv-run-tests run tests --only env-composition-helpers
```

Or run the standalone checks with an available nixpkgs source:

```sh
python3 tests/env-composition-helpers/check.py --nixpkgs /path/to/nixpkgs
```

An optional `--compat-nixpkgs /path/to/another/nixpkgs` repeats the pure evaluation checks against another library.
The real environment captures and compiler checks use the primary package set.
The runner needs Nix daemon access and builds the probe's dependencies if they are not already available.
Generated files live in a temporary directory.

The checks cover merge conflicts and ordering, setup timing, late hook precedence, value conversion,
literal shell metacharacters, trailing newlines, helper cleanup, JSON/Bash consistency, and two 70 KB values.
A C program verifies the custom compiler flag, zlib headers, linking, and runtime library discovery.

## Integration work before exposing this API

- Decide the public helper namespace and how module consumers should read helper-valued `config.env` entries.
  Current consumers expect ordinary values, so the prototype option type is isolated to this test.
- Replace the test's minimal stdenv setup wrapper with an integration that preserves stdenv's full interface.
  The wrapper exists only to test final operations after every hook, not to recommend a production adapter.
- Preserve existing derivation coercion, module priorities, dotenv ownership, `info` output and unset behavior.
- Decide whether plain values should ever change from setup inputs to final replacements, with migration guidance.
- Validate macOS SDK behavior and the supported Nix backends. The initial real capture checks were run on Linux.
