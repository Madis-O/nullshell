//! Translates keyboard and mouse input into the byte sequences a shell expects.

use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use crate::term::TermMode;

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn modifier_param(mods: ModifiersState) -> u8 {
    1 + mods.shift_key() as u8 + 2 * mods.alt_key() as u8 + 4 * mods.control_key() as u8
}

/// The C0 control code produced by Ctrl + `c`, if there is one.
fn control_code(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// Bytes to send for a key press, or `None` if the key produces nothing.
pub fn encode_key(event: &KeyEvent, mods: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    let (shift, alt, ctrl) = (mods.shift_key(), mods.alt_key(), mods.control_key());
    let m = modifier_param(mods);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    // ESC [ 1 ; m X  (or SS3 X in application cursor mode, when unmodified)
    let letter = |ch: char| -> Vec<u8> {
        if m == 1 {
            let intro = if app_cursor { "\x1bO" } else { "\x1b[" };
            format!("{intro}{ch}").into_bytes()
        } else {
            format!("\x1b[1;{m}{ch}").into_bytes()
        }
    };
    // ESC [ n ; m ~
    let tilde = |n: u8| -> Vec<u8> {
        if m == 1 { format!("\x1b[{n}~") } else { format!("\x1b[{n};{m}~") }.into_bytes()
    };
    // F1-F4 are SS3 P..S
    let ss3 = |ch: char| -> Vec<u8> {
        if m == 1 { format!("\x1bO{ch}") } else { format!("\x1b[1;{m}{ch}") }.into_bytes()
    };
    let with_alt = |bytes: &[u8]| -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if alt {
            out.push(0x1b);
        }
        out.extend_from_slice(bytes);
        out
    };

    if let Key::Named(named) = &event.logical_key {
        let bytes = match named {
            NamedKey::ArrowUp => letter('A'),
            NamedKey::ArrowDown => letter('B'),
            NamedKey::ArrowRight => letter('C'),
            NamedKey::ArrowLeft => letter('D'),
            NamedKey::Home => letter('H'),
            NamedKey::End => letter('F'),
            NamedKey::Insert => tilde(2),
            NamedKey::Delete => tilde(3),
            NamedKey::PageUp => tilde(5),
            NamedKey::PageDown => tilde(6),
            NamedKey::F1 => ss3('P'),
            NamedKey::F2 => ss3('Q'),
            NamedKey::F3 => ss3('R'),
            NamedKey::F4 => ss3('S'),
            NamedKey::F5 => tilde(15),
            NamedKey::F6 => tilde(17),
            NamedKey::F7 => tilde(18),
            NamedKey::F8 => tilde(19),
            NamedKey::F9 => tilde(20),
            NamedKey::F10 => tilde(21),
            NamedKey::F11 => tilde(23),
            NamedKey::F12 => tilde(24),
            NamedKey::Enter => with_alt(b"\r"),
            NamedKey::Backspace => with_alt(if ctrl { b"\x08" } else { b"\x7f" }),
            NamedKey::Tab => {
                if shift {
                    b"\x1b[Z".to_vec()
                } else {
                    with_alt(b"\t")
                }
            }
            NamedKey::Escape => with_alt(b"\x1b"),
            NamedKey::Space => with_alt(if ctrl { b"\0" } else { b" " }),
            _ => return event.text.as_ref().map(|t| t.as_bytes().to_vec()),
        };
        return Some(bytes);
    }

    if let Key::Character(s) = &event.logical_key {
        // AltGr arrives as Ctrl+Alt. It is AltGr (and not a real Ctrl+Alt
        // chord) when the modifiers changed which character the key gives.
        let altgr = ctrl
            && alt
            && matches!(event.key_without_modifiers(), Key::Character(base) if !base.eq_ignore_ascii_case(s));

        if ctrl && !altgr {
            let mut chars = s.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                if let Some(code) = control_code(c) {
                    return Some(with_alt(&[code]));
                }
            }
            return Some(with_alt(s.as_bytes()));
        }

        let text = event.text.as_ref().map(|t| t.as_str()).unwrap_or(s.as_str());
        if altgr {
            return Some(text.as_bytes().to_vec());
        }
        return Some(with_alt(text.as_bytes()));
    }

    event.text.as_ref().map(|t| t.as_bytes().to_vec())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButtonCode {
    Left = 0,
    Middle = 1,
    Right = 2,
    /// Pointer motion with no button held.
    None = 3,
    WheelUp = 64,
    WheelDown = 65,
}

/// Mouse report for programs that asked for mouse events (nvim, btop, ...).
/// `col` and `row` are zero based.
pub fn encode_mouse(
    button: MouseButtonCode,
    pressed: bool,
    motion: bool,
    col: usize,
    row: usize,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let mut code = button as u8;
    if motion {
        code += 32;
    }
    if mods.shift_key() {
        code += 4;
    }
    if mods.alt_key() {
        code += 8;
    }
    if mods.control_key() {
        code += 16;
    }

    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes());
    }

    // Legacy X10 encoding: one byte per value, release has no button.
    if !pressed {
        code = (code & !0b11) | 3;
    }
    if col >= 222 || row >= 222 {
        return None;
    }
    Some(vec![0x1b, b'[', b'M', 32 + code, 32 + col as u8 + 1, 32 + row as u8 + 1])
}

/// Text prepared for pasting into the shell.
pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // Strip escape characters so pasted text cannot end the bracket early.
        let clean: String = text.chars().filter(|c| *c != '\x1b' && *c != '\x03').collect();
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_codes() {
        assert_eq!(control_code('c'), Some(0x03));
        assert_eq!(control_code('C'), Some(0x03));
        assert_eq!(control_code('['), Some(0x1b));
        assert_eq!(control_code('1'), None);
    }

    #[test]
    fn sgr_mouse() {
        let mode = TermMode::SGR_MOUSE | TermMode::MOUSE_REPORT_CLICK;
        let bytes = encode_mouse(MouseButtonCode::Left, true, false, 4, 9, ModifiersState::empty(), mode);
        assert_eq!(bytes.unwrap(), b"\x1b[<0;5;10M");
        let bytes = encode_mouse(MouseButtonCode::WheelDown, true, false, 0, 0, ModifiersState::CONTROL, mode);
        assert_eq!(bytes.unwrap(), b"\x1b[<81;1;1M");
    }

    #[test]
    fn legacy_mouse_release() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        let bytes = encode_mouse(MouseButtonCode::Left, false, false, 0, 0, ModifiersState::empty(), mode);
        assert_eq!(bytes.unwrap(), vec![0x1b, b'[', b'M', 32 + 3, 33, 33]);
    }

    #[test]
    fn paste_modes() {
        assert_eq!(encode_paste("a\r\nb\nc", TermMode::NONE), b"a\rb\rc");
        assert_eq!(encode_paste("x\x1by", TermMode::BRACKETED_PASTE), b"\x1b[200~xy\x1b[201~");
    }
}
