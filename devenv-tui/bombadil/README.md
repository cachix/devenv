# Bombadil terminal fuzzing for devenv-tui

End-to-end fuzzing of the real TUI binary through a PTY, using
[bombadil](https://github.com/antithesishq/bombadil) (**experimental**). The
workflow pins a post-v0.6.1 upstream revision because released v0.6.1 can
sample a differential TUI halfway through a redraw. The pinned runner waits
for PTY output to become quiescent before extracting the next state.

This complements the in-process property tests in `../tests/tui_proptest.rs`:

| Layer | What it covers | Where |
|------|----------------|-------|
| Snapshots | exact rendered output for fixed inputs | `../tests/tui_tests.rs` |
| Property tests | generated model + `view()` invariants, including width-fit | `../tests/tui_proptest.rs` |
| Scripted PTY replay | exact key/resize/lifecycle regression with semantic command log | `../tests/replay-pty.sh` |
| Bombadil (this) | real PTY: crossterm input decode, ANSI output, SIGWINCH | here |

The property and scripted PTY tests are deterministic gates in the normal build
workflow. Bombadil runs nightly and on demand in `.github/workflows/tui-fuzz.yml`;
the workflow runs the deterministic replay corpus before random exploration.

## Running

```bash
# 1. Build the shared target and deterministic PTY driver.
cargo build -p devenv-run-tests -p devenv-tui \
  --features devenv-tui/deterministic-tui

# 2. Run the stable regression corpus first.
bash devenv-tui/tests/replay-pty.sh \
  target/debug/devenv-run-tests target/debug/tui-replay

# 3. Build the exact unmodified upstream revision used in CI.
bombadil_path=$(nix build --accept-flake-config --no-link --print-out-paths \
  github:antithesishq/bombadil/ddf7942fa502ca65b9f7b2d605fedf649b2528c2)

# 4. Fuzz the same reactive target.
"$bombadil_path/bin/bombadil" terminal test \
  --time-limit 60s --quiescence-timeout-ms 50 \
  --output-path /tmp/bombadil-out \
  --specification devenv-tui/bombadil/devenv-tui.spec.ts \
  -- ./target/debug/tui-replay --hold --attached --reactive \
    "$PWD/devenv-tui/replays/processes.jsonl"

# 5. Reproduce a failure.
"$bombadil_path/bin/bombadil" terminal test \
  --reproduce /tmp/bombadil-out --quiescence-timeout-ms 50 \
  --specification devenv-tui/bombadil/devenv-tui.spec.ts \
  -- ./target/debug/tui-replay --hold --attached --reactive \
    "$PWD/devenv-tui/replays/processes.jsonl"
```

Notes:

- The trace at `--output-path` records the full terminal grid per sampled
  state and can grow by hundreds of MB per minute. Delete it after triage.
- To make reproducers sound, keep the harness deterministic: build with
  `deterministic-tui` (static spinner/time) and use the fixed replay trace.
- Terminal-grid width is not a useful Bombadil property: an emulator's grid is
  width-bounded by construction. Pre-terminal overflow is enforced by the
  generated `rendered_lines_fit_usable_width` property instead. Bombadil checks
  observable crash/Unicode invariants and bounded process lifecycle recovery.
- Bombadil randomizes columns at a fixed 40 rows. Its emulator does not model
  row-count changes reliably for this scrollback-preserving, non-alternate-
  screen TUI. The scripted PTY regression covers narrow-to-wide row and
  column resizes; generated renderer tests cover degenerate sizes. Likewise,
  the spec sends PageUp and
  PageDown bytes instead of Bombadil's `ScrollUp`/`ScrollDown`, which move only
  the observer's scrollback viewport and do not exercise devenv input.
- Once a Bombadil failure is minimized, promote its meaningful key/resize
  sequence into `../tests/replay-pty.sh`. Screen assertions prove rendering;
  the JSONL semantic sidecar proves commands were neither dropped nor repeated.
