// Bombadil terminal specification for devenv-tui.
//
// This is the end-to-end layer that complements the deterministic property
// tests in `tests/tui_proptest.rs`. Those tests drive the model + `view()`
// in-process; this spec drives the *real binary* through a PTY, exercising
// crossterm input decoding, ANSI output, and SIGWINCH resize handling that an
// in-process test cannot reach.
//
// Status: EXPERIMENTAL. Pinned to the bombadil v0.6.0 terminal API
// (`@antithesishq/bombadil/terminal`). The terminal spec API is not yet
// documented upstream; the names below come from the published npm type
// definitions. Bombadil discovers exported action generators and properties
// from this module.
//
// Run against the long-lived replay harness (see ../README.md):
//
//   cargo build -p devenv-tui --features deterministic-tui --bin tui-replay
//   bombadil terminal test --time-limit 60s --output-path /tmp/bombadil-out \
//     --specification devenv-tui/bombadil/devenv-tui.spec.ts \
//     -- ./target/debug/tui-replay --hold --loop 0 devenv-tui/replays/quick-test

import { always, weighted, actions, integers } from "@antithesishq/bombadil";
import { extract, type State, type Action } from "@antithesishq/bombadil/terminal";
import {
  typeFromSet,
  CharSets,
  pasteText,
} from "@antithesishq/bombadil/terminal/defaults/actions";

// Send raw bytes the app actually reads. crossterm decodes these into key
// events, so control bytes map to devenv-tui's chords more reliably than
// PressKey codes.
const k = (bytes: string): Action => ({ TypeText: { text: bytes } });

// Random resize within the usable range (the in-process tests show iocraft
// itself cannot fit content below ~30 columns, so there is no point fuzzing
// narrower).
const resize = actions((): Action[] => [
  {
    Resize: {
      size: {
        columns: integers().min(40).max(220).generate(),
        rows: integers().min(2).max(60).generate(),
      },
    },
  },
]);

// The driver: weighted toward devenv-tui's real input vocabulary, with a slice
// of adversarial input for robustness.
export const drive = weighted<Action>([
  [8, k("\x1b[A")], // Up
  [8, k("\x1b[B")], // Down
  [6, k("j")],
  [6, k("k")],
  [6, k("\x05")], // Ctrl+E  expand logs
  [4, k("\x12")], // Ctrl+R  (re)start process
  [4, k("\x18")], // Ctrl+X  stop process
  [4, k("\x08")], // Ctrl+H  toggle hide-stopped
  [5, k("\x1b")], // Esc     clear selection / back
  [12, resize],
  [8, { ScrollUp: {} }],
  [8, { ScrollDown: {} }],
  [3, k("\x03")], // Ctrl+C  quit prompt
  [2, k("c")], // keep running
  [2, k("q")], // quit
  [3, typeFromSet(CharSets.CONTROL_ALL)], // input-parser robustness
  [2, typeFromSet(CharSets.UNICODE_CJK.union(CharSets.UNICODE_EMOTICONS))],
  [1, actions(() => [pasteText("A".repeat(8000))])], // huge paste
]);

// --- Properties -----------------------------------------------------------

// Largest non-empty cell column in a row, i.e. the rendered width of that row.
function rowWidth(s: State, r: number): number {
  const cells = s.grid.row(r);
  for (let c = cells.length - 1; c >= 0; c--) {
    if (cells[c].contents.trim() !== "") return c + 1;
  }
  return 0;
}

// No rendered row may extend past the terminal width. NOTE: this currently
// surfaces the known bottom nav/help-bar overflow that the in-process tests
// also document (see `rendered_lines_fit_usable_width`). Once the help bar gets
// a width budget this becomes a clean invariant.
const overflow = extract((s: State) => {
  for (let r = 0; r < s.grid.size.rows; r++) {
    if (rowWidth(s, r) > s.grid.size.columns) return true;
  }
  return false;
});
export const noOverflow = always(() => !overflow.current);

// devenv internals must never leak panic/debug text onto the screen.
const leaked = extract((s: State) => {
  for (let r = 0; r < s.grid.size.rows; r++) {
    if (/panicked|RUST_BACKTRACE|\{:\?\}/.test(s.grid.rowText(r))) return true;
  }
  return false;
});
export const noPanicText = always(() => !leaked.current);

// Crash/abort detection (a debug_assert abort shows up as a signal) and unicode
// width/decoding correctness come for free from the defaults.
export { exitSuccess, noReplacementChars } from "@antithesishq/bombadil/terminal/defaults/properties";
