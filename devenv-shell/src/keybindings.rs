use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShellAction {
    TogglePause,
    ListWatchedFiles,
    ToggleError,
    Reload,
}

impl ShellAction {
    pub const ALL: [Self; 4] = [
        Self::TogglePause,
        Self::ListWatchedFiles,
        Self::ToggleError,
        Self::Reload,
    ];

    pub const INTERCEPTED: [Self; 3] =
        [Self::TogglePause, Self::ListWatchedFiles, Self::ToggleError];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TogglePause => "toggle_pause",
            Self::ListWatchedFiles => "list_watched_files",
            Self::ToggleError => "toggle_error",
            Self::Reload => "reload",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShellKeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Space,
    Function(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShellKeyChord {
    pub code: ShellKeyCode,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

impl ShellKeyChord {
    pub fn new(code: ShellKeyCode, control: bool, alt: bool, shift: bool) -> Self {
        Self {
            code,
            control,
            alt,
            shift,
        }
    }

    pub fn terminal_bytes(&self) -> Vec<u8> {
        match self.code {
            ShellKeyCode::Char(character) => self.character_bytes(character),
            ShellKeyCode::Space => self.character_bytes(' '),
            ShellKeyCode::Enter => self.simple_bytes(b'\r'),
            ShellKeyCode::Esc => self.simple_bytes(0x1b),
            ShellKeyCode::Backspace => self.simple_bytes(0x7f),
            ShellKeyCode::Tab if self.shift => self.backtab_bytes(),
            ShellKeyCode::Tab => self.simple_bytes(b'\t'),
            ShellKeyCode::BackTab => self.backtab_bytes(),
            ShellKeyCode::Up => self.modified_csi(b'A'),
            ShellKeyCode::Down => self.modified_csi(b'B'),
            ShellKeyCode::Right => self.modified_csi(b'C'),
            ShellKeyCode::Left => self.modified_csi(b'D'),
            ShellKeyCode::Home => self.modified_csi(b'H'),
            ShellKeyCode::End => self.modified_csi(b'F'),
            ShellKeyCode::Insert => self.modified_tilde(2),
            ShellKeyCode::Delete => self.modified_tilde(3),
            ShellKeyCode::PageUp => self.modified_tilde(5),
            ShellKeyCode::PageDown => self.modified_tilde(6),
            ShellKeyCode::Function(number) => self.function_bytes(number),
        }
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(match self.code {
            ShellKeyCode::Char(character) if self.control || self.alt || self.shift => {
                character.to_ascii_uppercase().to_string()
            }
            _ => self.code_label(),
        });
        parts.join("+")
    }

    pub fn compact_label(&self) -> String {
        let mut label = String::new();
        if self.control {
            label.push('^');
        }
        if self.alt {
            label.push('⌥');
        }
        if self.shift {
            label.push('⇧');
        }
        match self.code {
            ShellKeyCode::Char(character) if self.control || self.alt || self.shift => {
                label.push(character.to_ascii_uppercase());
            }
            _ => label.push_str(&self.code_label()),
        }
        label
    }

    pub fn escaped_bytes(&self) -> String {
        self.terminal_bytes()
            .into_iter()
            .map(|byte| format!("\\x{byte:02x}"))
            .collect()
    }

    pub fn nushell_modifier(&self) -> &'static str {
        match (self.control, self.alt, self.shift) {
            (false, false, false) => "none",
            (true, false, false) => "control",
            (false, true, false) => "alt",
            (false, false, true) => "shift",
            (true, true, false) => "control_alt",
            (true, false, true) => "control_shift",
            (false, true, true) => "shift_alt",
            (true, true, true) => "control_alt_shift",
        }
    }

    pub fn nushell_keycode(&self) -> String {
        match self.code {
            ShellKeyCode::Char(character) => format!("char_{character}"),
            ShellKeyCode::Space => "char_ ".to_string(),
            ShellKeyCode::Enter => "enter".to_string(),
            ShellKeyCode::Esc => "esc".to_string(),
            ShellKeyCode::Backspace => "backspace".to_string(),
            ShellKeyCode::Delete => "delete".to_string(),
            ShellKeyCode::Insert => "insert".to_string(),
            ShellKeyCode::Up => "up".to_string(),
            ShellKeyCode::Down => "down".to_string(),
            ShellKeyCode::Left => "left".to_string(),
            ShellKeyCode::Right => "right".to_string(),
            ShellKeyCode::Home => "home".to_string(),
            ShellKeyCode::End => "end".to_string(),
            ShellKeyCode::PageUp => "pageup".to_string(),
            ShellKeyCode::PageDown => "pagedown".to_string(),
            ShellKeyCode::Tab => "tab".to_string(),
            ShellKeyCode::BackTab => "backtab".to_string(),
            ShellKeyCode::Function(number) => format!("f{number}"),
        }
    }

    fn character_bytes(&self, character: char) -> Vec<u8> {
        let emitted = if self.shift {
            shifted_character(character)
        } else {
            character
        };
        let mut bytes = if self.control {
            let Some(byte) = control_byte(emitted) else {
                return self.csi_u(character);
            };
            vec![byte]
        } else {
            let mut buffer = [0; 4];
            emitted.encode_utf8(&mut buffer).as_bytes().to_vec()
        };
        if self.alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    }

    fn simple_bytes(&self, byte: u8) -> Vec<u8> {
        if self.control || self.shift {
            return format!("\x1b[{byte};{}u", self.xterm_modifier(false)).into_bytes();
        }
        let mut bytes = vec![byte];
        if self.alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    }

    fn modified_csi(&self, final_byte: u8) -> Vec<u8> {
        let modifier = self.xterm_modifier(false);
        if modifier == 1 {
            vec![0x1b, b'[', final_byte]
        } else {
            format!("\x1b[1;{modifier}{}", final_byte as char).into_bytes()
        }
    }

    fn backtab_bytes(&self) -> Vec<u8> {
        if !self.control && !self.alt {
            b"\x1b[Z".to_vec()
        } else {
            let modifier = self.xterm_modifier(true);
            format!("\x1b[1;{modifier}Z").into_bytes()
        }
    }

    fn modified_tilde(&self, code: u8) -> Vec<u8> {
        let modifier = self.xterm_modifier(false);
        if modifier == 1 {
            format!("\x1b[{code}~").into_bytes()
        } else {
            format!("\x1b[{code};{modifier}~").into_bytes()
        }
    }

    fn function_bytes(&self, number: u8) -> Vec<u8> {
        let modifier = self.xterm_modifier(false);
        if number <= 4 {
            let final_byte = b'P' + number.saturating_sub(1);
            if modifier == 1 {
                vec![0x1b, b'O', final_byte]
            } else {
                format!("\x1b[1;{modifier}{}", final_byte as char).into_bytes()
            }
        } else {
            self.function_tilde_code(number)
                .map(|code| {
                    if modifier == 1 {
                        format!("\x1b[{code}~").into_bytes()
                    } else {
                        format!("\x1b[{code};{modifier}~").into_bytes()
                    }
                })
                .unwrap_or_default()
        }
    }

    fn function_tilde_code(&self, number: u8) -> Option<u8> {
        Some(match number {
            5 => 15,
            6 => 17,
            7 => 18,
            8 => 19,
            9 => 20,
            10 => 21,
            11 => 23,
            12 => 24,
            13 => 25,
            14 => 26,
            15 => 28,
            16 => 29,
            17 => 31,
            18 => 32,
            19 => 33,
            20 => 34,
            21 => 42,
            22 => 43,
            23 => 44,
            24 => 45,
            _ => return None,
        })
    }

    fn xterm_modifier(&self, implicit_shift: bool) -> u8 {
        1 + u8::from(self.shift || implicit_shift)
            + 2 * u8::from(self.alt)
            + 4 * u8::from(self.control)
    }

    fn csi_u(&self, character: char) -> Vec<u8> {
        format!("\x1b[{};{}u", character as u32, self.xterm_modifier(false)).into_bytes()
    }

    fn code_label(&self) -> String {
        match self.code {
            ShellKeyCode::Char(character) => character.to_string(),
            ShellKeyCode::Enter => "Enter".to_string(),
            ShellKeyCode::Esc => "Esc".to_string(),
            ShellKeyCode::Backspace => "Backspace".to_string(),
            ShellKeyCode::Delete => "Delete".to_string(),
            ShellKeyCode::Insert => "Insert".to_string(),
            ShellKeyCode::Up => "↑".to_string(),
            ShellKeyCode::Down => "↓".to_string(),
            ShellKeyCode::Left => "←".to_string(),
            ShellKeyCode::Right => "→".to_string(),
            ShellKeyCode::Home => "Home".to_string(),
            ShellKeyCode::End => "End".to_string(),
            ShellKeyCode::PageUp => "PageUp".to_string(),
            ShellKeyCode::PageDown => "PageDown".to_string(),
            ShellKeyCode::Tab => "Tab".to_string(),
            ShellKeyCode::BackTab => "Shift+Tab".to_string(),
            ShellKeyCode::Space => "Space".to_string(),
            ShellKeyCode::Function(number) => format!("F{number}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShellKeybindings {
    bindings: BTreeMap<ShellAction, Vec<ShellKeyChord>>,
    reload_overridden: bool,
}

impl Default for ShellKeybindings {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        for (action, character) in [
            (ShellAction::TogglePause, 'd'),
            (ShellAction::ListWatchedFiles, 'w'),
            (ShellAction::ToggleError, 'e'),
            (ShellAction::Reload, 'r'),
        ] {
            bindings.insert(
                action,
                vec![ShellKeyChord::new(
                    ShellKeyCode::Char(character),
                    true,
                    true,
                    false,
                )],
            );
        }
        Self {
            bindings,
            reload_overridden: false,
        }
    }
}

impl ShellKeybindings {
    pub fn replace(&mut self, action: ShellAction, bindings: Vec<ShellKeyChord>) {
        if action == ShellAction::Reload {
            self.reload_overridden = true;
        }
        self.bindings.insert(action, bindings);
    }

    pub fn bindings(&self, action: ShellAction) -> &[ShellKeyChord] {
        self.bindings.get(&action).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn reload_bindings(&self, dialect: &str) -> &[ShellKeyChord] {
        if !self.reload_overridden && dialect == "bash" {
            &[]
        } else {
            self.bindings(ShellAction::Reload)
        }
    }

    pub fn reload_overridden(&self) -> bool {
        self.reload_overridden
    }

    pub fn action_for_input(&self, input: &[u8]) -> Option<ShellAction> {
        ShellAction::INTERCEPTED.into_iter().find(|action| {
            self.bindings(*action)
                .iter()
                .any(|binding| binding.terminal_bytes() == input)
        })
    }

    pub fn conflict(&self) -> Option<ShellKeybindingConflict> {
        let mut seen: Vec<(ShellAction, &ShellKeyChord, Vec<u8>)> = Vec::new();
        for action in ShellAction::ALL {
            for binding in self.bindings(action) {
                let bytes = binding.terminal_bytes();
                if let Some((existing_action, existing, _)) =
                    seen.iter().find(|(_, _, existing)| {
                        *existing == bytes
                            || existing.starts_with(&bytes)
                            || bytes.starts_with(existing)
                    })
                {
                    let existing_label = existing.label();
                    let label = binding.label();
                    return Some(ShellKeybindingConflict {
                        first: *existing_action,
                        second: action,
                        chord: if existing_label == label {
                            label
                        } else {
                            format!("{existing_label} / {label}")
                        },
                    });
                }
                seen.push((action, binding, bytes));
            }
        }
        None
    }

    pub fn key_label(&self, action: ShellAction, compact: bool) -> Option<String> {
        let bindings = self.bindings(action);
        if compact {
            bindings
                .iter()
                .map(ShellKeyChord::compact_label)
                .min_by_key(String::len)
        } else {
            bindings.first().map(ShellKeyChord::label)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellKeybindingConflict {
    pub first: ShellAction,
    pub second: ShellAction,
    pub chord: String,
}

fn control_byte(character: char) -> Option<u8> {
    match character {
        ' ' | '@' => Some(0),
        'a'..='z' => Some(character as u8 - b'a' + 1),
        'A'..='Z' => Some(character as u8 - b'A' + 1),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(0x7f),
        _ => None,
    }
}

fn shifted_character(character: char) -> char {
    match character {
        '`' => '~',
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        _ => character.to_ascii_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_session_bindings_match_existing_terminal_bytes() {
        let bindings = ShellKeybindings::default();
        assert_eq!(
            bindings.bindings(ShellAction::TogglePause)[0].terminal_bytes(),
            [0x1b, 0x04]
        );
        assert_eq!(
            bindings.bindings(ShellAction::ListWatchedFiles)[0].terminal_bytes(),
            [0x1b, 0x17]
        );
        assert_eq!(
            bindings.bindings(ShellAction::ToggleError)[0].terminal_bytes(),
            [0x1b, 0x05]
        );
    }

    #[test]
    fn modified_named_keys_use_xterm_sequences() {
        let chord = ShellKeyChord::new(ShellKeyCode::Up, true, true, false);
        assert_eq!(chord.terminal_bytes(), b"\x1b[1;7A");
        let chord = ShellKeyChord::new(ShellKeyCode::Function(12), false, false, false);
        assert_eq!(chord.terminal_bytes(), b"\x1b[24~");
    }

    #[test]
    fn shifted_ascii_characters_match_terminal_input() {
        let chord = ShellKeyChord::new(ShellKeyCode::Char('/'), false, false, true);
        assert_eq!(chord.terminal_bytes(), b"?");
        let chord = ShellKeyChord::new(ShellKeyCode::Char('1'), false, true, true);
        assert_eq!(chord.terminal_bytes(), b"\x1b!");
        let chord = ShellKeyChord::new(ShellKeyCode::BackTab, false, false, false);
        assert_eq!(chord.terminal_bytes(), b"\x1b[Z");
    }

    #[test]
    fn reload_defaults_preserve_each_dialect() {
        let bindings = ShellKeybindings::default();
        assert!(bindings.reload_bindings("bash").is_empty());
        assert_eq!(bindings.reload_bindings("fish").len(), 1);
        assert_eq!(bindings.reload_bindings("nu").len(), 1);
        assert_eq!(bindings.reload_bindings("zsh").len(), 1);
    }

    #[test]
    fn explicit_reload_binding_applies_to_bash() {
        let mut bindings = ShellKeybindings::default();
        bindings.replace(
            ShellAction::Reload,
            vec![ShellKeyChord::new(
                ShellKeyCode::Function(12),
                false,
                false,
                false,
            )],
        );
        assert_eq!(bindings.reload_bindings("bash").len(), 1);
    }
}
