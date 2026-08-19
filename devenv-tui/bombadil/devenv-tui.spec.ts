// Bombadil terminal specification for devenv-tui.
//
// This is the end-to-end layer that complements the deterministic property
// tests in `tests/tui_proptest.rs`. Those tests drive the model + `view()`
// in-process; this spec drives the *real binary* through a PTY, exercising
// crossterm input decoding, ANSI output, and SIGWINCH resize handling that an
// in-process test cannot reach.
//
// Status: EXPERIMENTAL. Pinned to a post-v0.6.1 bombadil revision
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
//     -- ./target/debug/tui-replay --hold --attached --reactive \
//       devenv-tui/replays/processes.jsonl

import {
  always,
  eventually,
  now,
} from "@antithesishq/bombadil";
import { CharSet } from "@antithesishq/bombadil/actions";
import {
  actions,
  extract,
  type ActionTemplate,
  type State,
  weighted,
} from "@antithesishq/bombadil/terminal";

// Send raw bytes the app actually reads. crossterm decodes these into key
// events, so control bytes map to devenv-tui's chords more reliably than
// PressKey codes.
const k = (bytes: string): ActionTemplate => ({
  TypeText: { CharSet: CharSet.fromLiterals(bytes) },
});

const screen = extract((s: State) => {
  const rows: string[] = [];
  for (let r = 0; r < s.grid.size.rows; r++) rows.push(s.grid.rowText(r));
  return rows.join("\n");
});

type ProcessName = "api" | "worker" | "disabled";
type VisibleStatus = string | null;

// devenv deliberately preserves terminal history. After a resize, the active
// grid can contain an older frame above the current one, so later matches win.
const processStates = extract((s: State): Record<ProcessName, VisibleStatus> => {
  const states: Record<ProcessName, VisibleStatus> = {
    api: null,
    worker: null,
    disabled: null,
  };
  for (let r = 0; r < s.grid.size.rows; r++) {
    const row = s.grid.rowText(r);
    for (const process of Object.keys(states) as ProcessName[]) {
      if (!row.includes(process)) continue;
      const status = row.match(
        /\b(restarting|stopping|stopped|ready|running|starting|waiting|exited)\b/,
      );
      if (status) states[process] = status[1];
    }
  }
  return states;
});

type View = "main" | "expanded" | "prompt" | "unknown";
const currentView = (): View => {
  const markers: [View, number][] = [
    ["main", screen.current.lastIndexOf("↑↓ nav")],
    ["expanded", screen.current.lastIndexOf("j/k:line")],
    ["prompt", screen.current.lastIndexOf("Detach or stop")],
  ];
  const [view, index] = markers.reduce((latest, entry) =>
    entry[1] > latest[1] ? entry : latest,
  );
  return index >= 0 ? view : "unknown";
};

const mainView = () =>
  currentView() === "main";
const expandedView = () => currentView() === "expanded";
const interruptPrompt = () => currentView() === "prompt";
const appReady = () => mainView() || expandedView() || interruptPrompt();
const whenReady = (action: ActionTemplate) =>
  actions((): ActionTemplate[] =>
    appReady() ? [action] : [],
  );
const whenMain = (action: ActionTemplate) =>
  actions((): ActionTemplate[] =>
    mainView() ? [action] : [],
  );
// The terminal action API has no Wait variant. With the pinned runner's PTY
// quiescence timeout, an empty write advances time without busy-spinning or
// interfering with crossterm's startup capability probes.
const waitForTui = actions((): ActionTemplate[] =>
  !appReady() ? [k("")] : [],
);

// Randomize width while retaining enough rows for a complete current frame.
// Bombadil's emulator cannot reliably model row-count changes for a
// scrollback-preserving, non-alternate-screen TUI. The scripted real-PTY test
// covers 48x12 -> 120x40 and the in-process properties cover all small sizes.
const resize = actions((): ActionTemplate[] =>
  appReady()
    ? [
        {
          Resize: {
            columns: [60, 220],
            rows: 40,
          },
        },
      ]
    : [],
);

const restartInFlight = () =>
  Object.values(processStates.current).includes("restarting");
const restartProcess = actions((): ActionTemplate[] =>
  mainView() && !restartInFlight() ? [k("\x12")] : [],
);
const restartBurst = actions((): ActionTemplate[] =>
  mainView() && !restartInFlight() ? [k("\x12".repeat(64))] : [],
);
const stopProcess = actions((): ActionTemplate[] =>
  mainView() ? [k("\x18")] : [],
);
const openInterruptPrompt = actions((): ActionTemplate[] =>
  appReady() && !interruptPrompt() ? [k("\x03")] : [],
);
const dismissInterruptPrompt = actions((): ActionTemplate[] =>
  interruptPrompt() ? [k("\x1b"), k("c")] : [],
);

// The driver: weighted toward devenv-tui's real input vocabulary, with a slice
// of adversarial input for robustness.
export const drive = weighted([
  [1, waitForTui],
  [8, whenReady(k("\x1b[A"))], // Up
  [8, whenReady(k("\x1b[B"))], // Down
  [6, whenReady(k("j"))],
  [6, whenReady(k("k"))],
  [4, whenMain(k("/api\r"))],
  [4, whenMain(k("\r"))],
  [6, whenMain(k("\x05"))], // Ctrl+E  expand logs
  [4, restartProcess], // Ctrl+R  (re)start process
  [4, stopProcess], // Ctrl+X  stop process
  // A valid-input burst must be coalesced or rejected safely while the backend
  // catches up; user-controlled terminal input can never be a panic condition.
  [2, restartBurst],
  [4, whenMain(k("\x08"))], // Ctrl+H  toggle hide-stopped
  [5, whenReady(k("\x1b"))], // Esc     clear selection / back
  [12, resize],
  // Exercise the attached interrupt prompt but never choose `s` or Ctrl-C from
  // inside it, both of which intentionally terminate the target.
  [2, openInterruptPrompt],
  [4, dismissInterruptPrompt],
  // Fixed adversarial representatives keep startup gating explicit. Random
  // navigation above composes them into arbitrary byte sequences over time.
  [3, whenReady(k("\x1b[5~\x1b[6~"))], // PageUp + PageDown parser robustness
  [2, whenReady(k("界🙂e\u0301"))],
  [1, whenReady(k("\x1b[200~" + "A".repeat(512) + "\x1b[201~"))],
]);

// --- Properties -----------------------------------------------------------

// devenv internals must never leak panic/debug text onto the screen.
const leaked = extract((s: State) => {
  for (let r = 0; r < s.grid.size.rows; r++) {
    if (/panicked|RUST_BACKTRACE|\{:\?\}/.test(s.grid.rowText(r))) return true;
  }
  return false;
});
export const noPanicText = always(() => !leaked.current);

// Prevent an empty/dead harness from making every other property pass
// vacuously. This also gives crossterm time to finish terminal negotiation
// before any generated input is allowed.
export const fixtureAppears = eventually(mainView).within(2, "seconds");

const restartTarget: Record<ProcessName, VisibleStatus> = {
  api: "ready",
  worker: "running",
  disabled: "running",
};

const restartCompletes = (process: ProcessName) =>
  always(
    now(() => processStates.current[process] === "restarting").implies(
      eventually(() => {
        const state = processStates.current[process];
        // The terminal cannot observe a process row while another view is on
        // screen or Ctrl-H has hidden it. A stop also legitimately supersedes
        // an in-flight restart and has its own completion property below.
        return !mainView() || state === null ||
          state === restartTarget[process] ||
          state === "stopping" || state === "stopped";
      }).within(2, "seconds"),
    ),
  );

const stopCompletes = (process: ProcessName) =>
  always(
    now(() => processStates.current[process] === "stopping").implies(
      eventually(() => {
        const state = processStates.current[process];
        // Ctrl-H may hide a process as soon as it becomes stopped.
        return state === "stopped" || state === null;
      }).within(2, "seconds"),
    ),
  );

export const apiRestartCompletes = restartCompletes("api");
export const workerRestartCompletes = restartCompletes("worker");
export const disabledRestartCompletes = restartCompletes("disabled");
export const apiStopCompletes = stopCompletes("api");
export const workerStopCompletes = stopCompletes("worker");
export const disabledStopCompletes = stopCompletes("disabled");

// Crash/abort detection (a debug_assert abort shows up as a signal) and unicode
// width/decoding correctness come for free from the defaults.
export { exitSuccess, noReplacementChars } from "@antithesishq/bombadil/terminal/defaults/properties";
