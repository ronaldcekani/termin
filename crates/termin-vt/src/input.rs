//! Key and mouse encoding according to active terminal modes.
//!
//! See `docs/specs/terminal-core.md §10`.

use smallvec::SmallVec;
use termin_core::{Key, KeyEvent, Mods, MouseButton, MouseKind};

use crate::cell::AbsPos;
use crate::modes::{KittyKeyboard, Modes, OptionAsAlt};

/// Mouse event translated to cell coordinates.
///
/// See `docs/specs/terminal-core.md §10`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CellMouseEvent {
    pub pos: AbsPos,
    pub event: termin_core::MouseEvent,
}

/// Encode key event into escape sequence bytes.
pub fn encode_key(
    ev: &KeyEvent,
    modes: Modes,
    kitty: &KittyKeyboard,
    modify_other_keys: u8,
    option_as_alt: OptionAsAlt,
) -> SmallVec<[u8; 16]> {
    let mut out = SmallVec::new();

    let kitty_flags = kitty.current_flags();
    if kitty_flags != 0 {
        // Encode via kitty keyboard protocol
        encode_kitty_key(ev, kitty_flags, &mut out);
        return out;
    }

    let mod_num = compute_xterm_mod(ev.mods);

    if modify_other_keys == 2 {
        if let Key::Char(c) = ev.key {
            if ev.mods.contains(Mods::CTRL) || ev.mods.contains(Mods::ALT) {
                // CSI 27 ; mod ; code ~
                let s = format!("\x1b[27;{};{}~", mod_num, c as u32);
                out.extend_from_slice(s.as_bytes());
                return out;
            }
        }
    }

    match ev.key {
        Key::Char(c) => {
            let alt_active = match option_as_alt {
                OptionAsAlt::None => false,
                OptionAsAlt::Both => ev.mods.contains(Mods::ALT),
                OptionAsAlt::Left => ev.mods.contains(Mods::ALT), // macOS option
                OptionAsAlt::Right => ev.mods.contains(Mods::ALT),
            };

            if ev.mods.contains(Mods::CTRL) {
                let ctrl_byte = match c {
                    '@'..='_' => (c as u8) & 0x1f,
                    'a'..='z' => (c as u8) - b'a' + 1,
                    '?' => 127,
                    _ => c as u8,
                };
                if alt_active {
                    out.push(b'\x1b');
                }
                out.push(ctrl_byte);
            } else if alt_active {
                out.push(b'\x1b');
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            } else if let Some(ref text) = ev.text {
                out.extend_from_slice(text.as_bytes());
            } else {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }

        Key::Named(named) => {
            encode_named_key(named, ev.mods, mod_num, modes, &mut out);
        }
    }

    out
}

fn compute_xterm_mod(mods: Mods) -> u8 {
    let mut val = 1;
    if mods.contains(Mods::SHIFT) {
        val += 1;
    }
    if mods.contains(Mods::ALT) {
        val += 2;
    }
    if mods.contains(Mods::CTRL) {
        val += 4;
    }
    if mods.contains(Mods::CMD) {
        val += 8;
    }
    val
}

fn encode_named_key(
    named: termin_core::NamedKey,
    mods: Mods,
    mod_num: u8,
    modes: Modes,
    out: &mut SmallVec<[u8; 16]>,
) {
    use termin_core::NamedKey::*;

    let decckm = modes.contains(Modes::DECCKM);
    let has_mods = mod_num > 1;

    match named {
        Enter => {
            if modes.contains(Modes::LINE_FEED_NEW_LINE) {
                out.extend_from_slice(b"\r\n");
            } else {
                out.push(b'\r');
            }
        }
        Tab => {
            if mods.contains(Mods::SHIFT) {
                out.extend_from_slice(b"\x1b[Z"); // Backtab (CBT)
            } else {
                out.push(b'\t');
            }
        }
        Backspace => {
            out.push(0x7f);
        }
        Escape => {
            out.push(0x1b);
        }
        Space => {
            out.push(b' ');
        }
        ArrowUp => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[1;{}A", mod_num).as_bytes());
            } else if decckm {
                out.extend_from_slice(b"\x1bOA");
            } else {
                out.extend_from_slice(b"\x1b[A");
            }
        }
        ArrowDown => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[1;{}B", mod_num).as_bytes());
            } else if decckm {
                out.extend_from_slice(b"\x1bOB");
            } else {
                out.extend_from_slice(b"\x1b[B");
            }
        }
        ArrowRight => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[1;{}C", mod_num).as_bytes());
            } else if decckm {
                out.extend_from_slice(b"\x1bOC");
            } else {
                out.extend_from_slice(b"\x1b[C");
            }
        }
        ArrowLeft => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[1;{}D", mod_num).as_bytes());
            } else if decckm {
                out.extend_from_slice(b"\x1bOD");
            } else {
                out.extend_from_slice(b"\x1b[D");
            }
        }
        Home => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[1;{}H", mod_num).as_bytes());
            } else if decckm {
                out.extend_from_slice(b"\x1bOH");
            } else {
                out.extend_from_slice(b"\x1b[H");
            }
        }
        End => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[1;{}F", mod_num).as_bytes());
            } else if decckm {
                out.extend_from_slice(b"\x1bOF");
            } else {
                out.extend_from_slice(b"\x1b[F");
            }
        }
        PageUp => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[5;{}~", mod_num).as_bytes());
            } else {
                out.extend_from_slice(b"\x1b[5~");
            }
        }
        PageDown => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[6;{}~", mod_num).as_bytes());
            } else {
                out.extend_from_slice(b"\x1b[6~");
            }
        }
        Insert => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[2;{}~", mod_num).as_bytes());
            } else {
                out.extend_from_slice(b"\x1b[2~");
            }
        }
        Delete => {
            if has_mods {
                out.extend_from_slice(format!("\x1b[3;{}~", mod_num).as_bytes());
            } else {
                out.extend_from_slice(b"\x1b[3~");
            }
        }
        F(1) => out.extend_from_slice(b"\x1bOP"),
        F(2) => out.extend_from_slice(b"\x1bOQ"),
        F(3) => out.extend_from_slice(b"\x1bOR"),
        F(4) => out.extend_from_slice(b"\x1bOS"),
        F(5) => out.extend_from_slice(b"\x1b[15~"),
        F(6) => out.extend_from_slice(b"\x1b[17~"),
        F(7) => out.extend_from_slice(b"\x1b[18~"),
        F(8) => out.extend_from_slice(b"\x1b[19~"),
        F(9) => out.extend_from_slice(b"\x1b[20~"),
        F(10) => out.extend_from_slice(b"\x1b[21~"),
        F(11) => out.extend_from_slice(b"\x1b[23~"),
        F(12) => out.extend_from_slice(b"\x1b[24~"),
        _ => {}
    }
}

fn encode_kitty_key(ev: &KeyEvent, _flags: u8, out: &mut SmallVec<[u8; 16]>) {
    let key_num = match ev.key {
        Key::Char(c) => c as u32,
        Key::Named(termin_core::NamedKey::Enter) => 13,
        Key::Named(termin_core::NamedKey::Tab) => 9,
        Key::Named(termin_core::NamedKey::Backspace) => 127,
        Key::Named(termin_core::NamedKey::Escape) => 27,
        _ => 0,
    };

    let mod_num = compute_xterm_mod(ev.mods);
    let s = format!("\x1b[{};{}u", key_num, mod_num);
    out.extend_from_slice(s.as_bytes());
}

/// Encode cell mouse event according to active mouse modes.
pub fn encode_mouse(ev: &CellMouseEvent, modes: Modes) -> Option<SmallVec<[u8; 16]>> {
    // Check if any mouse mode is active
    let is_x10 = modes.contains(Modes::MOUSE_X10);
    let is_normal = modes.contains(Modes::MOUSE_NORMAL);
    let is_button = modes.contains(Modes::MOUSE_BUTTON);
    let is_any = modes.contains(Modes::MOUSE_ANY);

    if !is_x10 && !is_normal && !is_button && !is_any {
        return None;
    }

    let is_sgr = modes.contains(Modes::MOUSE_SGR);

    let (btn_code, is_release) = match ev.event.kind {
        MouseKind::Down(btn) => (mouse_button_code(btn), false),
        MouseKind::Up(btn) => (mouse_button_code(btn), true),
        MouseKind::Drag(btn) => (mouse_button_code(btn) + 32, false),
        MouseKind::Move => {
            if !is_any {
                return None;
            }
            (35, false)
        }
    };

    let col = ev.pos.col + 1; // 1-based
    let row = (ev.pos.line.0 + 1) as u32; // 1-based

    let mut out = SmallVec::new();
    if is_sgr {
        let action = if is_release { 'm' } else { 'M' };
        let s = format!("\x1b[<{};{};{}{}", btn_code, col, row, action);
        out.extend_from_slice(s.as_bytes());
    } else {
        // Legacy X10/Normal
        if is_release && is_x10 {
            return None;
        }
        let cb = if is_release { 3 } else { btn_code } + 32;
        let cx = (col as u32 + 32).min(255) as u8;
        let cy = (row + 32).min(255) as u8;
        out.extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
    }

    Some(out)
}

fn mouse_button_code(btn: MouseButton) -> u8 {
    match btn {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Other(n) => 3 + n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::{NamedKey, Point};

    #[test]
    fn test_encode_key_decckm() {
        let ev = KeyEvent {
            key: Key::Named(NamedKey::ArrowUp),
            physical: 0,
            mods: Mods::empty(),
            text: None,
            repeat: false,
            pressed: true,
        };

        let kitty = KittyKeyboard::new();

        // Normal mode -> CSI A
        let res1 = encode_key(&ev, Modes::empty(), &kitty, 0, OptionAsAlt::Both);
        assert_eq!(&res1[..], b"\x1b[A");

        // DECCKM -> SS3 A
        let res2 = encode_key(&ev, Modes::DECCKM, &kitty, 0, OptionAsAlt::Both);
        assert_eq!(&res2[..], b"\x1bOA");
    }

    #[test]
    fn test_encode_mouse_sgr() {
        let ev = CellMouseEvent {
            pos: AbsPos {
                line: crate::cell::AbsLine(4),
                col: 9,
            },
            event: termin_core::MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                pos: Point { x: 0.0, y: 0.0 },
                mods: Mods::empty(),
                click_count: 1,
            },
        };

        let modes = Modes::MOUSE_NORMAL | Modes::MOUSE_SGR;
        let encoded = encode_mouse(&ev, modes).unwrap();
        assert_eq!(std::str::from_utf8(&encoded).unwrap(), "\x1b[<0;10;5M");
    }
}
