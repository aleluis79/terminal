//! Keyboard input handling.
//!
//! Converts keyboard events to terminal input bytes.
//! Handles special keys (arrows, function keys, modifiers).

/// A keyboard event that produces terminal input bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    /// Regular character key.
    Char {
        /// The character.
        c: char,
        /// Whether Ctrl is pressed.
        ctrl: bool,
        /// Whether Alt/Option is pressed.
        alt: bool,
    },
    /// Arrow key.
    Arrow {
        /// Arrow direction.
        direction: ArrowDirection,
        /// Whether Ctrl is pressed.
        ctrl: bool,
        /// Whether Alt/Option is pressed.
        alt: bool,
    },
    /// Function key.
    Function {
        /// Function key number (1-24).
        num: u8,
        /// Whether Ctrl is pressed.
        ctrl: bool,
        /// Whether Alt/Option is pressed.
        alt: bool,
    },
    /// Special key.
    Special {
        /// The special key pressed.
        key: SpecialKey,
        /// Whether Ctrl is pressed.
        ctrl: bool,
        /// Whether Alt/Option is pressed.
        alt: bool,
    },
}

/// Arrow key direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowDirection {
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
}

/// Special keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKey {
    /// Escape key.
    Escape,
    /// Backspace key.
    Backspace,
    /// Tab key.
    Tab,
    /// Enter/Return key.
    Enter,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
    /// Delete key.
    Delete,
    /// Insert key.
    Insert,
}

/// Convert a keyboard event to terminal input bytes.
#[allow(clippy::too_many_lines)]
pub fn key_event_to_bytes(event: &KeyEvent) -> Vec<u8> {
    match event {
        KeyEvent::Char { c, ctrl, alt } => {
            if *ctrl && c.is_ascii() {
                // Ctrl + ASCII char: convert to ASCII control code
                let code = *c as u32;
                if (0x40..0x60).contains(&code) {
                    // A-Z → 0x01-0x1A
                    vec![(code - 0x40) as u8]
                } else if (0x60..0x80).contains(&code) {
                    // a-z → 0x01-0x1A
                    vec![(code - 0x60) as u8]
                } else {
                    // Other chars with Ctrl
                    vec![(code & 0x1f) as u8]
                }
            } else if *alt {
                // Alt + char: ESC followed by the UTF-8 encoded character
                let mut bytes = vec![0x1b];
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                bytes
            } else {
                // Plain char, encoded as UTF-8 (may be multi-byte, e.g. 'ñ')
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyEvent::Arrow {
            direction,
            ctrl,
            alt,
        } => {
            let mut bytes = vec![0x1b, b'['];
            // Modifier flags: 3 = ctrl, 5 = alt
            // Only add modifiers if at least one is set
            let has_mod = *ctrl || *alt;
            if has_mod {
                bytes.push(b'1');
                bytes.push(b';');
                if *ctrl {
                    bytes.push(b'3');
                }
                if *alt {
                    if *ctrl {
                        bytes.push(b';');
                    }
                    bytes.push(b'5');
                }
            }
            match direction {
                ArrowDirection::Up => bytes.push(b'A'),
                ArrowDirection::Down => bytes.push(b'B'),
                ArrowDirection::Right => bytes.push(b'C'),
                ArrowDirection::Left => bytes.push(b'D'),
            }
            bytes
        }
        KeyEvent::Function { num, ctrl, alt } => {
            let mut bytes = vec![0x1b, b'O'];
            if *alt {
                bytes.push(b'1');
                bytes.push(b';');
                bytes.push(b'3');
                bytes.push(b';');
            }
            if *ctrl {
                bytes.push(b'1');
                bytes.push(b';');
                bytes.push(b'5');
                bytes.push(b';');
            }
            bytes.push(b'P' + *num - 1);
            bytes
        }
        KeyEvent::Special { key, ctrl, alt } => {
            let mut bytes = vec![0x1b, b'['];
            if *alt {
                bytes.push(b'1');
                bytes.push(b';');
                bytes.push(b'3');
            }
            if *ctrl {
                bytes.push(b'1');
                bytes.push(b';');
                bytes.push(b'5');
            }
            match key {
                SpecialKey::Escape => {
                    // RF-68: Escape is a bare `ESC`; there is no `CSI` form for
                    // it, so modifiers are ignored.
                    return vec![0x1b];
                }
                SpecialKey::Backspace => {
                    // `DEL` (0x7f) is the conventional byte for Backspace, and
                    // the one `cmd.exe` needs: with a raw `BS` (0x08) `ConPTY`
                    // sees Ctrl+H and makes `cmd.exe` delete the whole word
                    // instead of a single character. `BS` is kept for the
                    // explicit Ctrl+Backspace chord, where deleting the word is
                    // the expected behaviour.
                    if *ctrl {
                        return vec![0x08];
                    }
                    return vec![0x7f];
                }
                SpecialKey::Tab => {
                    if *ctrl {
                        return vec![0x1b, b'[', b'I'];
                    }
                    return vec![0x09];
                }
                SpecialKey::Enter => return vec![0x0d],
                SpecialKey::Home => bytes.push(b'H'),
                SpecialKey::End => bytes.push(b'F'),
                SpecialKey::PageUp => {
                    bytes.clear();
                    bytes.extend(vec![0x1b, b'[', b'5', b'~']);
                }
                SpecialKey::PageDown => {
                    bytes.clear();
                    bytes.extend(vec![0x1b, b'[', b'6', b'~']);
                }
                SpecialKey::Delete => {
                    // RF-69: `ESC [ 3 ~`, with the modifier after the `3`
                    // (`ESC [ 3 ; N ~`). xterm's modifier code is
                    // `1 + alt*2 + ctrl*4`, so Alt=3, Ctrl=5, both=7. The
                    // generic `ESC[`+modifier prefix cannot express this.
                    bytes.clear();
                    bytes.extend_from_slice(&[0x1b, b'[', b'3']);
                    let mut modifier = 1u8;
                    if *alt {
                        modifier += 2;
                    }
                    if *ctrl {
                        modifier += 4;
                    }
                    if modifier > 1 {
                        bytes.push(b';');
                        bytes.push(b'0' + modifier);
                    }
                    bytes.push(b'~');
                }
                SpecialKey::Insert => {
                    bytes.clear();
                    bytes.extend(vec![0x1b, b'[', b'2', b'~']);
                }
            }
            bytes
        }
    }
}

/// Encode text for pasting into the terminal.
///
/// Line feeds become carriage returns, which is what the terminal line
/// discipline expects from a paste. When `bracketed` is true the payload is
/// wrapped in `ESC[200~` … `ESC[201~` so applications can tell a paste apart
/// from typed input.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");

    let mut bytes = Vec::with_capacity(normalized.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(normalized.as_bytes());
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paste_bytes_plain_text_is_unchanged() {
        assert_eq!(paste_bytes("hello", false), b"hello");
    }

    #[test]
    fn test_paste_bytes_newlines_become_carriage_returns() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb");
    }

    #[test]
    fn test_paste_bytes_crlf_is_not_doubled() {
        assert_eq!(paste_bytes("a\r\nb", false), b"a\rb");
    }

    #[test]
    fn test_paste_bytes_bracketed_is_wrapped() {
        assert_eq!(paste_bytes("hi", true), b"\x1b[200~hi\x1b[201~");
    }

    #[test]
    fn test_paste_bytes_bracketed_multiline() {
        assert_eq!(
            paste_bytes("a\nb", true),
            b"\x1b[200~a\rb\x1b[201~",
            "bracketed paste must still normalise newlines"
        );
    }

    #[test]
    fn test_paste_bytes_empty() {
        assert_eq!(paste_bytes("", false), b"");
        assert_eq!(paste_bytes("", true), b"\x1b[200~\x1b[201~");
    }

    #[test]
    fn test_paste_bytes_unicode_is_utf8() {
        assert_eq!(paste_bytes("ñ", false), vec![0xc3, 0xb1]);
    }

    #[test]
    fn test_char_plain() {
        let event = KeyEvent::Char {
            c: 'a',
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![b'a']);
    }

    #[test]
    fn test_char_ctrl() {
        let event = KeyEvent::Char {
            c: 'c',
            ctrl: true,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x03]); // Ctrl+C = 0x03
    }

    #[test]
    fn test_char_alt() {
        let event = KeyEvent::Char {
            c: 'a',
            ctrl: false,
            alt: true,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'a']);
    }

    #[test]
    fn test_char_plain_multibyte() {
        // Non-ASCII characters must be sent as UTF-8, not truncated to a byte.
        let event = KeyEvent::Char {
            c: 'ñ',
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0xc3, 0xb1]);
    }

    #[test]
    fn test_char_plain_three_bytes() {
        let event = KeyEvent::Char {
            c: '€',
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0xe2, 0x82, 0xac]);
    }

    #[test]
    fn test_char_alt_multibyte() {
        let event = KeyEvent::Char {
            c: 'á',
            ctrl: false,
            alt: true,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, 0xc3, 0xa1]);
    }

    #[test]
    fn test_char_ctrl_non_ascii_is_utf8() {
        // Ctrl only maps to control codes for ASCII; otherwise send the char.
        let event = KeyEvent::Char {
            c: 'ñ',
            ctrl: true,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0xc3, 0xb1]);
    }

    #[test]
    fn test_char_bytes_roundtrip_through_parser() {
        // What the keyboard produces must decode back to the same character.
        let event = KeyEvent::Char {
            c: 'ñ',
            ctrl: false,
            alt: false,
        };
        let bytes = key_event_to_bytes(&event);
        let mut parser = crate::parser::Parser::new();
        assert_eq!(
            parser.parse_bytes(&bytes),
            vec![crate::parser::Action::Write('ñ')]
        );
    }

    #[test]
    fn test_arrow_up() {
        let event = KeyEvent::Arrow {
            direction: ArrowDirection::Up,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'A']);
    }

    #[test]
    fn test_arrow_down() {
        let event = KeyEvent::Arrow {
            direction: ArrowDirection::Down,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'B']);
    }

    #[test]
    fn test_backspace() {
        let event = KeyEvent::Special {
            key: SpecialKey::Backspace,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x7f]);
    }

    #[test]
    fn test_ctrl_backspace() {
        // Ctrl+Backspace keeps the control byte so that `ConPTY`/`cmd.exe`
        // deletes the previous word.
        let event = KeyEvent::Special {
            key: SpecialKey::Backspace,
            ctrl: true,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x08]);
    }

    #[test]
    fn test_enter() {
        let event = KeyEvent::Special {
            key: SpecialKey::Enter,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x0d]);
    }

    #[test]
    fn test_tab() {
        let event = KeyEvent::Special {
            key: SpecialKey::Tab,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x09]);
    }

    #[test]
    fn test_home() {
        let event = KeyEvent::Special {
            key: SpecialKey::Home,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'H']);
    }

    #[test]
    fn test_delete() {
        let event = KeyEvent::Special {
            key: SpecialKey::Delete,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'3', b'~']);
    }

    #[test]
    fn test_delete_with_modifiers() {
        // CL-30: the modifier goes after the `3`, so `ESC[3;N~`.
        let alt = KeyEvent::Special {
            key: SpecialKey::Delete,
            ctrl: false,
            alt: true,
        };
        assert_eq!(key_event_to_bytes(&alt), b"\x1b[3;3~".to_vec());

        let ctrl = KeyEvent::Special {
            key: SpecialKey::Delete,
            ctrl: true,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&ctrl), b"\x1b[3;5~".to_vec());

        let both = KeyEvent::Special {
            key: SpecialKey::Delete,
            ctrl: true,
            alt: true,
        };
        assert_eq!(key_event_to_bytes(&both), b"\x1b[3;7~".to_vec());
    }

    #[test]
    fn test_delete_is_not_the_malformed_sequence() {
        // CL-30: `ESC[~` is not a valid Delete.
        let event = KeyEvent::Special {
            key: SpecialKey::Delete,
            ctrl: false,
            alt: false,
        };
        assert_ne!(key_event_to_bytes(&event), vec![0x1b, b'[', b'~']);
    }

    #[test]
    fn test_escape() {
        // RF-68: Escape is a single ESC byte.
        let event = KeyEvent::Special {
            key: SpecialKey::Escape,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b]);
    }

    #[test]
    fn test_escape_with_modifiers_is_still_esc() {
        // RF-68 / CL-30: Escape has no CSI form, so modifiers are ignored and
        // `ESC[[D` is never produced.
        for (ctrl, alt) in [(true, false), (false, true), (true, true)] {
            let event = KeyEvent::Special {
                key: SpecialKey::Escape,
                ctrl,
                alt,
            };
            assert_eq!(
                key_event_to_bytes(&event),
                vec![0x1b],
                "ctrl={ctrl} alt={alt}"
            );
        }
    }

    #[test]
    fn test_page_up() {
        let event = KeyEvent::Special {
            key: SpecialKey::PageUp,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'5', b'~']);
    }

    #[test]
    fn test_page_down() {
        let event = KeyEvent::Special {
            key: SpecialKey::PageDown,
            ctrl: false,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'6', b'~']);
    }

    #[test]
    fn test_arrow_with_modifiers() {
        let event = KeyEvent::Arrow {
            direction: ArrowDirection::Up,
            ctrl: true,
            alt: true,
        };
        assert_eq!(
            key_event_to_bytes(&event),
            vec![0x1b, b'[', b'1', b';', b'3', b';', b'5', b'A']
        );
    }

    #[test]
    fn test_ctrl_d() {
        // Ctrl+D = 0x04 (EOF)
        let event = KeyEvent::Char {
            c: 'd',
            ctrl: true,
            alt: false,
        };
        assert_eq!(key_event_to_bytes(&event), vec![0x04]);
    }
}
