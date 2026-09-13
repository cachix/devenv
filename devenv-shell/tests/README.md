The interactive CLI smoke test runs the actual devenv executable on a controlling
PTY, with its default reload session and TUI enabled. It covers initial sizes
80x24, 0x0, 0x24 and 80x0, command execution before and after resizing, exit code
propagation, and auto-activation through the bash prompt hook.

Run it from the repository root after building the PTY driver:

```sh
devenv shell -- cargo build -p devenv-run-tests
nix build .#devenv
bash devenv-shell/tests/smoke.sh \
  "$PWD/result/bin/devenv" "$PWD/target/debug/devenv-run-tests" \
  "$PWD/src/modules" /tmp/devenv-shell-smoke
```

The script uses a temporary project and isolated devenv configuration/trust
directories, reuses this checkout's nixpkgs lock, and retains transcripts in the
output directory. Expectations are bounded by a timeout and commands are sent
once. Computed output markers distinguish command execution from terminal echo.

The reusable build workflow runs this test against the default and static Nix
packages on Linux and macOS, before release pinning. Failures upload transcripts.
When diagnosing a release regression, run against the affected packaged binary
as well as the fixed candidate: a local Cargo build may use different native
libraries than a downstream package.
