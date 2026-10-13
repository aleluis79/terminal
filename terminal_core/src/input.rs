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
            if *ctrl {
                // Ctrl + char: convert to ASCII control code
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
                // Alt + char: ESC + char
                let mut bytes = vec![0x1b];
                bytes.push(*c as u8);
                bytes
            } else {
                // Plain char
                vec![*c as u8]
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
                    bytes.push(b'[');
                    bytes.push(b'D');
                }
                SpecialKey::Backspace => {
                    if *ctrl {
                        return vec![0x7f];
                    }
                    return vec![0x08];
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
                SpecialKey::Delete => bytes.push(b'~'),
                SpecialKey::Insert => {
                    bytes.clear();
                    bytes.extend(vec![0x1b, b'[', b'2', b'~']);
                }
            }
            bytes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(key_event_to_bytes(&event), vec![0x1b, b'[', b'~']);
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
