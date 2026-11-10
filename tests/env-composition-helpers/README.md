# Environment composition tests

These tests exercise the public `config.lib.env` helpers. See
[the environment variable documentation](../../docs/src/content/docs/environment-variables.mdx) for the API and merge rules.

Run the full integration fixture from the repository development shell:

```sh
devenv-run-tests run tests --only env-composition-helpers
```

The fixture checks dotenv precedence, setup inputs, late package hooks, explicit empty values,
configured-value references between modules, `devenv info`, captured JSON, and sourceable CLI activation.
It enters Bash, zsh, fish, and Nushell through a pseudo-terminal and checks helper values,
composition, metadata cleanup, and precedence of `enterShell` and task exports.
It also loads and unloads the environment through direnv, checking restoration of existing
values and removal of variables that were previously unset.
On macOS, a separate profile checks explicit `DEVELOPER_DIR` and `SDKROOT` values in both
captured JSON and an activated CLI shell. These are test paths, not a real Xcode installation;
the test verifies override precedence rather than compatibility with a particular SDK.
It also runs the standalone checks below.

```sh
python3 tests/env-composition-helpers/check.py --nixpkgs /path/to/nixpkgs
```

The optional `--compat-nixpkgs /path/to/another/nixpkgs` repeats pure module evaluation against another library.
Real captures and compiler checks use the primary package set. The runner needs Nix daemon access;
it builds dependencies that are not already available. Generated files live in a temporary directory.

The standalone checks cover:

- Module ordering, override priorities, conflicts, and ordinary configured values.
- Initial values during setup and final values after package and global hooks.
- Replacement, fallback, prepend and append behavior, including empty values and lists.
- Shell quoting, literal metacharacters, trailing newlines, and helper cleanup.
- JSON and Bash consistency, including values normally filtered by activation.
- Two 70 KB values without a single oversized environment string.
- Structured derivation attributes and `overrideAttrs` compatibility.
- Chained `override`, `overrideAttrs`, and `overrideDerivation` calls.
- Header discovery, a custom compiler flag, linking, and execution against zlib.
- Exact final PATH replacement without breaking normal shell builds.
- Retention of a dependency referenced only by a final helper value.

The PR test workflow runs this fixture in its own job on Linux and macOS,
so an unrelated failure in the full suite cannot prevent these checks from running.
This job uses read-only repository permissions and has no cache upload credentials.
It is separate from the reusable release test workflow.

The standalone probe sources exclude live `.devenv` state. Compiler subprocesses
use a live temporary directory instead of temporary paths from Nix's raw capture.
