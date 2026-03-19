use iocraft::prelude::{KeyCode, KeyEvent, KeyModifiers};

pub const INPUT_TOGGLE_HINT: &str = "F12";

pub fn is_input_toggle(key_event: &KeyEvent) -> bool {
    matches!(key_event.code, KeyCode::F(12))
}

pub fn encode_key_event(key_event: &KeyEvent) -> Option<Vec<u8>> {
    let alt = key_event.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);

    let mut bytes = match key_event.code {
        KeyCode::Char(c) => {
            if ctrl {
                vec![ctrl_char(c)?]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        _ => return None,
    };

    if alt && !matches!(key_event.code, KeyCode::Esc) {
        bytes.insert(0, 0x1b);
    }

    Some(bytes)
}

fn ctrl_char(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some((c as u8) - b'a' + 1),
        'A'..='Z' => Some((c as u8) - b'A' + 1),
        ' ' | '@' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(0x7f),
        _ => None,
    }
}
