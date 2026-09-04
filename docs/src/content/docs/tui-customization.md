---
title: "TUI customization"
---

The devenv TUI reads personal settings from a versioned YAML file. These settings affect presentation and interaction only. They are separate from the reproducible project configuration in `devenv.nix` and `devenv.yaml`.

The default path is `$XDG_CONFIG_HOME/devenv/config.yaml`, falling back to `~/.config/devenv/config.yaml`. This path is the same on Linux, macOS, and Linux distributions running under WSL.

Override it for one invocation with `--user-config` or for a shell with `DEVENV_USER_CONFIG`:

```sh
$ devenv --user-config ./demo.yaml up
$ DEVENV_USER_CONFIG=./demo.yaml devenv up
```

The flag takes precedence over the environment variable. Relative paths are resolved from the directory in which devenv was invoked, even when devenv later discovers and enters a project root. An explicitly selected file must exist. A missing default file uses the built-in settings.

## Start a configuration

Every file declares its schema version. Unknown fields, invalid colors, unsupported placeholders, duplicate statusline components, conflicting keys, and ambiguous key-sequence prefixes are errors.

```yaml
# yaml-language-server: $schema=https://devenv.sh/devenv.user.schema.json

version: 1
```

The `yaml-language-server` comment associates the file with the generated [JSON Schema](/devenv.user.schema.json) for editor completion and validation of static structure, ranges, action names, colors, and key syntax. devenv ignores it as ordinary YAML commentary. `user-config validate` additionally checks relationships between settings, custom names, key conflicts, and statusline formats. The CLI can inspect the active path and configuration without loading a project:

```sh
$ devenv user-config path
$ devenv user-config validate
$ devenv user-config show
$ devenv user-config schema
```

`user-config validate` validates the file at the resolved path. `user-config show` prints structural defaults and configured overrides. Empty keybinding tables inherit the effective defaults listed below.

## Complete example

This example keeps the TUI at the command's terminal position, adds profiles and the project name to the main statusline, colors profiles with a custom palette entry, moves the activity summary, uses a two-key shortcut in the log viewer, and changes log behavior.

```yaml
# yaml-language-server: $schema=https://devenv.sh/devenv.user.schema.json

version: 1
tui:
  viewport: inline
  theme:
    preset: devenv
    palette:
      profile: "#cba6f7"
      surface: ansi:236
    styles:
      statusline:
        background: surface
      statusline.profiles:
        foreground: profile
        modifiers: [bold]
  statusline:
    position: inline
    layouts:
      main:
        left: [profiles, summary]
        center: [project]
        right: [elapsed, key_hints]
    components:
      profiles:
        format: "profile {profiles}"
        compact_format: "{profiles}"
        priority: 90
      elapsed:
        format: "elapsed {elapsed}"
        compact_format: "{elapsed}"
        priority: 20
        overflow: hide
  keybindings:
    sequence_timeout_ms: 750
    logs:
      top: [home, "g g"]
      bottom: [end, "shift+g"]
  behavior:
    mouse: true
    follow_logs: true
    hide_stopped_processes: false
    log_preview_lines: 12
    log_history_lines: 10000
```

## Viewport placement

`tui.viewport` accepts `inline` (the default) or `top`. `inline` begins rendering where devenv was invoked and preserves the terminal content above it. The viewport stays there while it fits, then scrolls upward only by the rows needed when its content reaches the terminal bottom. This matches ordinary terminal output.

`top` moves the existing visible terminal content into scrollback and claims the terminal from its first row. It is useful when you prefer a stable top-aligned workspace regardless of where devenv was invoked:

```yaml
tui:
  viewport: top
```

## Statusline layouts

`tui.statusline.position` accepts `inline` (the default), `top`, or `bottom`. `inline` keeps devenv's normal terminal behavior by placing the statusline directly after the current activity output without reserving the terminal height. `top` and `bottom` are sticky modes that pin the statusline to the chosen terminal edge as activities expand or collapse. Fullscreen logs always fill the terminal. In fullscreen logs, `inline` uses the conventional bottom footer.

Each TUI mode has independent `left`, `center`, and `right` component lists:

```yaml
tui:
  statusline:
    layouts:
      main:
        left: [profiles, summary]
        center: [project]
        right: [elapsed, key_hints]
      logs:
        left: [log_mode, log_position]
        center: [project]
        right: [retained_logs, key_hints]
      search:
        left: [search]
        center: []
        right: [pending_key, key_hints]
      prompt:
        left: [prompt]
        center: []
        right: [key_hints]
```

The built-in components are:

| Component | Available values |
| --- | --- |
| `summary` | `{summary}` |
| `builds`, `downloads`, `queries`, `tasks` | `{active}`, `{completed}`, `{failed}`, `{total}`, `{expected}` |
| `processes` | `{running}`, `{stopped}`, `{failed}`, `{hidden}`, `{total}` |
| `profiles` | `{profiles}`, `{count}` |
| `project` | `{name}`, `{path}` |
| `command` | `{command}` |
| `shell` | `{shell}` |
| `elapsed` | `{elapsed}` |
| `selected` | `{name}`, `{status}` |
| `log_mode` | `{mode}` |
| `log_position` | `{current}`, `{total}`, `{percent}` |
| `retained_logs` | `{retained}`, `{discarded}`, `{total}` |
| `search` | `{query}`, `{current}`, `{total}`, `{result}` |
| `prompt` | `{prompt}` |
| `pending_key` | `{keys}` |
| `key_hints` | `{hints}` |

Components with no runtime value are omitted unless `show_empty: true`. Profiles are the fully resolved profiles for the current invocation, including command-line and trusted auto-activation profiles.

Each built-in component can be customized under `tui.statusline.components.<name>`. A custom name must declare a `type`:

```yaml
tui:
  statusline:
    components:
      brand:
        type: text
        text: devenv
        format: "[{text}]"
        required: true
      workspace:
        type: project
        format: "project {name}"
        compact_format: "{name}"
        priority: 80
        max_width: 30
        overflow: truncate
```

`format` is used first. If the terminal is too narrow, the renderer tries `compact_format`, then removes lower-priority components whose `overflow` is `hide`, then truncates by terminal display width. A larger `priority` preserves a component longer. `required` prevents the hide step, but still permits final truncation so the TUI never writes beyond the terminal width. Literal braces are written as `{{` and `}}`.

`max_width` limits a component to that many terminal columns before overflow handling. It must be at least 1. `type: text` requires `text`, and `text` is only valid for text components. `overflow` accepts `hide` or `truncate`.

Set `tui.statusline.enabled: false` to hide ordinary TUI statusline content and the persistent statusline in an interactive `devenv shell`. Search and Ctrl-C confirmation prompts remain visible in activity views so active interactions and their actions are never hidden. Set `tui.statusline.position` to `top` or `bottom` for a sticky statusline, or leave it as `inline` to keep it directly after activity output. The separator must be single-line text no wider than eight terminal columns.

## Colors and styles

The `devenv` preset retains the built-in palette. `terminal` uses terminal-native colors where possible, and `none` removes preset statusline colors. Explicit styles work with every preset. Theme customization currently applies to the statusline. Activity rows, process states, and log content retain their built-in semantic colors.

Colors accept named terminal colors such as `yellow`, `dark_grey`, and `default`, indexed colors as `ansi:0` through `ansi:255`, RGB colors as `#RRGGBB`, or a name from `tui.theme.palette`.

Styles can target the entire statusline, its separator, or one component:

```yaml
tui:
  theme:
    styles:
      statusline:
        foreground: default
      statusline.separator:
        foreground: dark_grey
      statusline.profiles:
        foreground: magenta
        background: ansi:236
        modifiers: [bold, underline]
```

Component-local `style` settings take precedence over theme scopes. Supported modifiers are `bold`, `dim`, `italic`, `underline`, and `reverse`.

## Keybindings

Keybindings are grouped by interaction context. Assigning an empty array disables an action's defaults.

```yaml
tui:
  keybindings:
    main:
      move_down: [down, j]
      move_up: [up, k]
      open_logs: ["ctrl+e", "g l"]
      toggle_stopped: []
```

Chords use lowercase modifiers in the order `ctrl`, `alt`, `shift`, followed by a key. Named keys include `enter`, `esc`, `backspace`, `delete`, `insert`, arrow keys, `home`, `end`, `page_up`, `page_down`, `tab`, `back_tab`, `space`, and `f1` through `f24`. Separate chords in a sequence with spaces. Sequences can contain up to four chords.

`tui.keybindings.sequence_timeout_ms` sets how long devenv waits for the next chord in a sequence. It accepts 100 through 5000 milliseconds and defaults to 750.

The contexts and actions are:

| Context | Actions |
| --- | --- |
| `main` | `move_down`, `move_up`, `half_page_down`, `half_page_up`, `activate`, `expand`, `collapse`, `open_logs`, `search`, `restart_process`, `stop_process`, `toggle_stopped`, `cancel` |
| `process_search` | `next_match`, `previous_match`, `accept`, `cancel` |
| `logs` | `line_down`, `line_up`, `half_page_down`, `half_page_up`, `page_down`, `page_up`, `top`, `bottom`, `search`, `next_match`, `previous_match`, `copy`, `back` |
| `log_search` | `accept`, `cancel` |
| `prompt` | `cancel`, `quit`, `stop_manager` |

Within one context, a chord cannot be assigned to two actions and a sequence cannot be the prefix of another sequence. `ctrl+c` is reserved for emergency interruption and copying selected log text, so it cannot be rebound.

Displayed key hints are derived from the resolved bindings. When a multi-key sequence is pending, the `pending_key` component can show its accepted prefix.

Terminal input protocols differ. Basic characters, arrows, and common control chords are broadly portable, while multiple modifiers and modified special keys require support from the terminal emulator. Validation proves that a binding is well-formed, not that every terminal can emit it distinctly.

### Default keybindings

```yaml
tui:
  keybindings:
    main:
      move_down: [down, j]
      move_up: [up, k]
      half_page_down: ["ctrl+d"]
      half_page_up: ["ctrl+u"]
      activate: [enter]
      expand: [right, l]
      collapse: [left, h]
      open_logs: ["ctrl+e"]
      search: ["/"]
      restart_process: ["ctrl+r"]
      stop_process: ["ctrl+x"]
      toggle_stopped: ["ctrl+h"]
      cancel: [esc]
    process_search:
      next_match: [down]
      previous_match: [up]
      accept: [enter]
      cancel: [esc]
    logs:
      line_down: [down, j]
      line_up: [up, k]
      half_page_down: ["ctrl+d"]
      half_page_up: ["ctrl+u"]
      page_down: [page_down, space, "ctrl+f"]
      page_up: [page_up, "ctrl+b"]
      top: [home, g]
      bottom: [end, "shift+g"]
      search: ["/"]
      next_match: [n]
      previous_match: ["shift+n"]
      copy: [y]
      back: [q, esc, "ctrl+e"]
    log_search:
      accept: [enter]
      cancel: [esc]
    prompt:
      cancel: [c, esc]
      quit: [q]
      stop_manager: [s]
```

## Shell keybindings

`shell.keybindings` controls shortcuts claimed inside an interactive `devenv shell`. Each action accepts a list of single key chords. Omit an action to keep its default. Set it to `[]` to release every shortcut for that action.

```yaml
shell:
  keybindings:
    toggle_pause: ["ctrl+alt+d"]
    list_watched_files: ["ctrl+alt+w"]
    toggle_error: ["ctrl+alt+e"]
    reload: ["ctrl+alt+r"]
```

Shell keybindings use the same key and modifier syntax as TUI keybindings. Multi-key sequences are not supported. `Ctrl+C` remains reserved.

`toggle_pause`, `list_watched_files`, and `toggle_error` have the same defaults in every shell. `reload` defaults to `Ctrl+Alt+R` in Fish, Nushell, and Zsh, and is unbound in Bash. An explicit `reload` binding applies to Bash, Fish, Nushell, and Zsh.

`DEVENV_RELOAD_KEYBIND` remains a Zsh-only fallback when `reload` is omitted.

## Behavior

```yaml
tui:
  behavior:
    mouse: true
    hide_stopped_processes: false
    follow_logs: true
    log_preview_lines: 10
    log_history_lines: 1000
```

- `mouse` enables selection and wheel scrolling in fullscreen logs.
- `hide_stopped_processes` controls the initial process filter.
- `follow_logs` controls the initial fullscreen log mode.
- `log_preview_lines` sets the maximum collapsed preview size from 1 through 1000.
- `log_history_lines` sets retained lines per build from `log_preview_lines` through 1,000,000.
