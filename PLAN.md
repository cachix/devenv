# Refactor raw ANSI escape codes to crossterm typed API

## Context

`devenv-shell/src/session.rs` uses raw ANSI escape codes (`\x1b[...`) throughout.
These are unreadable without a reference.
Crossterm (already a dependency) provides typed equivalents for most of them.

## Scope

File: `devenv-shell/src/session.rs`

### Convert to crossterm

Use `queue!()` (writes only, caller flushes) since most sites already do explicit flushes.

| Lines | Raw | Crossterm | Context |
|-------|-----|-----------|---------|
| 283 | `\x1b[{};1H\x1b[2K` | `MoveTo(0, row), Clear(ClearType::CurrentLine)` | `render()` |
| 285 | `\x1b[0m` | `ResetColor` | `render()` |
| 327 | `\x1b[{};1H\x1b[2K` | `MoveTo(0, row), Clear(ClearType::CurrentLine)` | `render_full()` |
| 329 | `\x1b[0m` | `ResetColor` | `render_full()` |
| 342 | `\x1b[?25h` | `cursor::Show` | `update_cursor()` |
| 344 | `\x1b[?25l` | `cursor::Hide` | `update_cursor()` |
| 348 | `\x1b[{};{}H` | `MoveTo(col-1, row-1)` | `update_cursor()` |
| 364 | `\x1b[{};{}H` | `MoveTo(col-1, row-1)` | `write_cursor()` |
| 839, 968, 1029 | `\x1b[?2026h` | `BeginSynchronizedUpdate` | event loop |
| 842, 1012, 1038 | `\x1b[?2026l` | `EndSynchronizedUpdate` | event loop |
| 876 | `\x1b[1;31m...\x1b[0m` | `SetAttribute(Bold), SetForegroundColor(Red)` / `ResetColor` | error display |
| 1120 | `\x1b[1m...\x1b[0m` | `SetAttribute(Bold)` / `ResetColor` | watched files |
| 1219 | `\x1b[?1049l` | `LeaveAlternateScreen` | cleanup |
| 1255 | `\x1b7...\x1b8` | `SavePosition` / `RestorePosition` | `clear_status_row()` |

### Keep as raw (no crossterm equivalent)

| Lines | Raw | Why |
|-------|-----|-----|
| 264 | `\x1b[1;{r}r` | Set scroll region (DECSTBM) — not in crossterm |
| 265 | `\x1b[{r};1H` | Part of scroll region setup, paired with 264 |
| 267 | `\x1b[r` | Reset scroll region — not in crossterm |
| 648 | `\x1b[r\x1b[?6l` | Reset scroll region + origin mode — not in crossterm |
| 1222 | `\x1b[?{}l` | Dynamic DEC private mode number — needs format string |

### Leave alone (not writing to terminal)

| Lines | Why |
|-------|-----|
| 579, 685 | `vt.feed_str(...)` — feeding the VT state machine, not the terminal |
| 39–70 | `dump_pen()` / `dump_color()` — building SGR from avt::Pen data, different concern |

### Cursor position query (line 606)

Keep as raw.
The code does custom stdin reading with guards for injected stdin.
Crossterm's `cursor::position()` wouldn't handle that case.

## Implementation

1. Add imports:
   ```rust
   use crossterm::{cursor, queue, style, terminal::{self, Clear, ClearType}};
   ```

2. Convert each site from the table above, using `queue!()` for sites that are followed by an explicit flush.

3. For the error/watched-files text formatting (lines 876, 1120), these embed escape codes in format strings that get fed to `feed_vt()` (the VT state machine), not written directly to stdout.
   They must stay as raw escape codes since crossterm's `queue!()` writes to a `Write` target, not a `String`.

## Verification

- `cargo check -p devenv-shell`
- `cargo nextest run -p devenv-shell`
