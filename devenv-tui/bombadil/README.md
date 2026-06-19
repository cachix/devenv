# Bombadil terminal fuzzing for devenv-tui

End-to-end fuzzing of the real TUI binary through a PTY, using
[bombadil](https://github.com/antithesishq/bombadil) (v0.6.0, **experimental**).

This complements the in-process property tests in `../tests/tui_proptest.rs`:

| Layer | What it covers | Where |
|------|----------------|-------|
| Snapshots | exact rendered output for fixed inputs | `../tests/tui_tests.rs` |
| Property tests | model + `view()` invariants, no-panic, width-fit | `../tests/tui_proptest.rs` |
| Bombadil (this) | real PTY: crossterm input decode, ANSI output, SIGWINCH | here |

The property tests are the fast, deterministic gate (run in `cargo nextest`).
Bombadil is the slow, end-to-end layer and is **not wired into CI** — it needs
the experimental bombadil binary vendored first.

## Running

```bash
# 1. Build the long-lived replay harness (deterministic output for reproducers).
cargo build -p devenv-tui --features deterministic-tui --bin tui-replay

# 2. Fuzz it. --hold keeps the TUI alive for input; --loop 0 replays forever
#    so there is always a live program under the fuzzer.
bombadil terminal test --time-limit 60s --output-path /tmp/bombadil-out \
  --specification devenv-tui/bombadil/devenv-tui.spec.ts \
  -- ./target/debug/tui-replay --hold --loop 0 devenv-tui/replays/quick-test

# 3. Reproduce a failure.
bombadil terminal test --reproduce /tmp/bombadil-out \
  --specification devenv-tui/bombadil/devenv-tui.spec.ts \
  -- ./target/debug/tui-replay --hold --loop 0 devenv-tui/replays/quick-test
```

Notes:

- The trace at `--output-path` records the full terminal grid per sampled
  state and grows fast (several GB per minute). Delete it after triage.
- `noOverflow` in the spec currently surfaces the known bottom nav/help-bar
  overflow that the in-process tests also document.
- To make reproducers sound, keep the harness deterministic: build with
  `deterministic-tui` (static spinner/time) and use the fixed replay trace.
