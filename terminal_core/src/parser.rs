//! ANSI/ECMA-48 escape sequence parser.
//!
//! This parser implements a finite state machine that processes raw byte
//! input and produces terminal actions (write character, set attribute,
//! move cursor, etc.).
//!
//! # Supported sequences
//!
//! - CSI (Control Sequence Introducer): `\x1b[`
//! - OSC (Operating System Command): `\x1b]`
//! - ESC single characters
//! - C0 and C1 control codes

use crate::cell::Color;
use crate::state::{CursorShape, TextAttributes};

/// An action produced by the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write a character at the current cursor position.
    Write(char),
    /// Move cursor to absolute position (1-indexed in spec, 0-indexed here).
    CursorPosition(usize, usize),
    /// Move cursor up n lines.
    CursorUp(usize),
    /// Move cursor down n lines.
    CursorDown(usize),
    /// Move cursor right n columns.
    CursorForward(usize),
    /// Move cursor left n columns.
    CursorBackward(usize),
    /// Move cursor to the given column, keeping the current row (CHA).
    CursorToColumn(usize),
    /// Send cursor to beginning of line.
    CursorHome,
    /// Clear from cursor to end of line.
    EraseInLine,
    /// Clear entire screen.
    EraseInDisplay,
    /// Insert n lines.
    InsertLines(usize),
    /// Delete n lines.
    DeleteLines(usize),
    /// Line feed.
    LineFeed,
    /// Carriage return.
    CarriageReturn,
    /// Backspace.
    Backspace,
    /// Set text attributes (SGR).
    SetAttributes(TextAttributes),
    /// Set foreground color.
    SetForegroundColor(Color),
    /// Set background color.
    SetBackgroundColor(Color),
    /// Reset attributes and colors.
    ResetAttributes,
    /// Enter alternate screen mode.
    EnterAlternateScreen,
    /// Exit alternate screen mode.
    ExitAlternateScreen,
    /// Enable bracketed paste mode.
    EnableBracketedPaste,
    /// Disable bracketed paste mode.
    DisableBracketedPaste,
    /// Bell (BEL character).
    Bell,
    /// OSC string with command and payload.
    OscCommand(u8, Vec<u8>),
    /// Set window title (OSC 2).
    SetWindowTitle(String),
    /// Set icon name (OSC 1).
    SetIconName(String),
    /// Set both title and icon name (OSC 0).
    SetWindowIconAndTitle(String),
    /// Set cursor shape.
    SetCursorShape(CursorShape),
    /// Move cursor to position (OSC 22).
    MoveCursorTo(usize, usize),
    /// Query foreground color (OSC 10) — GUI responds with current color.
    QueryForegroundColor,
    /// Query background color (OSC 11) — GUI responds with current color.
    QueryBackgroundColor,
    /// Query text color (OSC 12) — GUI responds with current color.
    QueryTextColor,
    /// Set foreground color (OSC 10).
    SetForegroundColorFromOsc(String),
    /// Set background color (OSC 11).
    SetBackgroundColorFromOsc(String),
    /// Set text color (OSC 12).
    SetTextColorFromOsc(String),
    /// Handle hyperlink (OSC 8).
    HyperlinkStart(String),
    /// End hyperlink (OSC 8).
    HyperlinkEnd,
    /// Unknown/unhandled sequence (logged but not crashed).
    Unknown,
}

/// The parser's internal state.
#[derive(PartialEq)]
enum ParserState {
    /// Normal mode: waiting for regular input or escape.
    Normal,
    /// Received ESC, waiting for next byte.
    Escape,
    /// Received ESC [, CSI prefix.
    Csi,
    /// Received ESC ], OSC prefix.
    Osc,
}

/// ANSI escape sequence parser.
pub struct Parser {
    state: ParserState,
    csi_params: Vec<u16>,
    csi_intermediates: Vec<u8>,
    osc_payload: Vec<u8>,
    utf8_buf: Vec<u8>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self {
            state: ParserState::Normal,
            csi_params: Vec::new(),
            csi_intermediates: Vec::new(),
            osc_payload: Vec::new(),
            utf8_buf: Vec::new(),
        }
    }

    /// Parse a single byte and produce actions.
    pub fn parse(&mut self, byte: u8) -> Vec<Action> {
        match self.state {
            ParserState::Normal => self.parse_normal(byte),
            ParserState::Escape => {
                let actions = self.parse_escape(byte);
                // Only reset to Normal if parse_escape didn't change state to Csi/Osc
                if self.state == ParserState::Escape {
                    self.state = ParserState::Normal;
                }
                actions
            }
            ParserState::Csi => self.parse_csi(byte),
            ParserState::Osc => self.parse_osc(byte),
        }
    }

    /// Parse bytes in bulk.
    pub fn parse_bytes(&mut self, bytes: &[u8]) -> Vec<Action> {
        let mut all_actions = Vec::new();
        for &byte in bytes {
            all_actions.extend(self.parse(byte));
        }
        all_actions
    }

    /// Reset the parser to its initial state.
    pub fn reset(&mut self) {
        self.state = ParserState::Normal;
        self.csi_params.clear();
        self.csi_intermediates.clear();
        self.osc_payload.clear();
        self.utf8_buf.clear();
    }

    fn parse_normal(&mut self, byte: u8) -> Vec<Action> {
        // Multi-byte UTF-8 sequences are accumulated until they decode.
        if byte >= 0x80 || !self.utf8_buf.is_empty() {
            if byte < 0x80 {
                // A pending sequence interrupted by an ASCII byte is invalid:
                // emit the replacement char and process the ASCII byte normally.
                self.utf8_buf.clear();
                let mut actions = vec![Action::Write('\u{FFFD}')];
                actions.extend(self.parse_ascii(byte));
                return actions;
            }
            return self.parse_utf8(byte);
        }
        self.parse_ascii(byte)
    }

    /// Accumulate one byte of a multi-byte UTF-8 sequence.
    fn parse_utf8(&mut self, byte: u8) -> Vec<Action> {
        self.utf8_buf.push(byte);
        match std::str::from_utf8(&self.utf8_buf) {
            Ok(text) => {
                let actions = text.chars().map(Action::Write).collect();
                self.utf8_buf.clear();
                actions
            }
            Err(error) => {
                if error.error_len().is_some() {
                    // Invalid sequence — replace with U+FFFD (CL-2).
                    self.utf8_buf.clear();
                    vec![Action::Write('\u{FFFD}')]
                } else {
                    // Incomplete sequence — wait for the next byte.
                    Vec::new()
                }
            }
        }
    }

    fn parse_ascii(&mut self, byte: u8) -> Vec<Action> {
        match byte {
            0x1b => {
                // ESC
                self.state = ParserState::Escape;
            }
            0x07 => {
                // BEL
                return vec![Action::Bell];
            }
            0x0a => {
                // LF
                return vec![Action::LineFeed];
            }
            0x0d => {
                // CR
                return vec![Action::CarriageReturn];
            }
            0x08 => {
                // BS
                return vec![Action::Backspace];
            }
            0x18 | 0x1a => {
                // CAN / SUB (legacy cancel)
                self.reset();
            }
            0x20..=0x7e => {
                // Printable ASCII
                return vec![Action::Write(byte as char)];
            }
            0x7f => {
                // DEL — replace with U+FFFD (CL-2)
                return vec![Action::Write('\u{FFFD}')];
            }
            _ => {
                // Other control codes — ignore.
            }
        }
        Vec::new()
    }

    fn parse_escape(&mut self, byte: u8) -> Vec<Action> {
        match byte {
            b'[' => {
                // ESC [ = CSI
                self.state = ParserState::Csi;
                self.csi_params.clear();
                self.csi_intermediates.clear();
                Vec::new()
            }
            b']' => {
                // ESC ] = OSC
                self.state = ParserState::Osc;
                self.osc_payload.clear();
                Vec::new()
            }
            b'c' => {
                // Full reset (RI)
                self.reset();
                vec![Action::ResetAttributes]
            }
            _ => {
                // Unknown ESC sequence
                Vec::new()
            }
        }
    }

    fn parse_csi(&mut self, byte: u8) -> Vec<Action> {
        if byte == 0x3f || byte == 0x3e {
            // Private markers '?' and '>' — skip, do not add as parameter
            Vec::new()
        } else if (0x3a..=0x3d).contains(&byte) {
            // Parameter separators (:, ;, <, =) — push 0 to start new param
            self.csi_params.push(0);
            Vec::new()
        } else if (0x30..=0x39).contains(&byte) {
            // Digits in parameter (e.g., CSI 1 ; 5 m)
            if let Some(last) = self.csi_params.last_mut() {
                *last = *last * 10 + u16::from(byte - b'0');
            } else {
                self.csi_params.push(u16::from(byte - b'0'));
            }
            Vec::new()
        } else if (0x20..=0x2f).contains(&byte) {
            // Intermediate byte
            self.csi_intermediates.push(byte);
            Vec::new()
        } else if (0x40..=0x7e).contains(&byte) {
            // Final byte — dispatch
            let actions = self.dispatch_csi(byte);
            self.csi_params.clear();
            self.csi_intermediates.clear();
            self.state = ParserState::Normal;
            actions
        } else {
            // Abort CSI
            self.csi_params.clear();
            self.csi_intermediates.clear();
            Vec::new()
        }
    }

    fn dispatch_csi(&self, final_byte: u8) -> Vec<Action> {
        if !self.csi_intermediates.is_empty() {
            return Vec::new();
        }

        let params = if self.csi_params.is_empty() {
            vec![0]
        } else {
            self.csi_params.clone()
        };

        match final_byte {
            // SGR (Select Graphic Rendition)
            b'm' => Parser::dispatch_sgr(&params),
            // CUU (Cursor Up)
            b'A' => vec![Action::CursorUp(
                params.first().copied().unwrap_or(1) as usize
            )],
            // CPL (Cursor Previous Line): up n lines and to column 0
            b'F' => vec![
                Action::CursorUp(params.first().copied().unwrap_or(1) as usize),
                Action::CursorToColumn(0),
            ],
            // CUD (Cursor Down)
            b'B' => vec![Action::CursorDown(
                params.first().copied().unwrap_or(1) as usize
            )],
            // CNL (Cursor Next Line): down n lines and to column 0
            b'E' => vec![
                Action::CursorDown(params.first().copied().unwrap_or(1) as usize),
                Action::CursorToColumn(0),
            ],
            // CUF (Cursor Forward)
            b'C' => vec![Action::CursorForward(
                params.first().copied().unwrap_or(1) as usize
            )],
            // CUB (Cursor Backward)
            b'D' => vec![Action::CursorBackward(
                params.first().copied().unwrap_or(1) as usize
            )],
            // CHA (Cursor Horizontal Absolute): move to column only, keep row
            b'G' => {
                let col = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                vec![Action::CursorToColumn(col)]
            }
            // CUP (Cursor Position)
            b'H' | b'f' => {
                let row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                vec![Action::CursorPosition(row, col)]
            }
            // ECH (Erase Character) / ED (Erase in Display)
            b'J' => match params.first().copied().unwrap_or(0) {
                0..=2 => vec![Action::EraseInDisplay],
                _ => Vec::new(),
            },
            // EL (Erase in Line)
            b'K' => match params.first().copied().unwrap_or(0) {
                0..=2 => vec![Action::EraseInLine],
                _ => Vec::new(),
            },
            // IL (Insert Line)
            b'L' => vec![Action::InsertLines(
                params.first().copied().unwrap_or(1) as usize
            )],
            // DL (Delete Line)
            b'M' => vec![Action::DeleteLines(
                params.first().copied().unwrap_or(1) as usize
            )],
            // DECSET (Set Mode)
            b'h' => Parser::dispatch_decset(&params),
            // DECRESET (Reset Mode)
            b'l' => Parser::dispatch_decreset(&params),
            _ => Vec::new(),
        }
    }

    fn dispatch_sgr(params: &[u16]) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut attrs = TextAttributes::default();
        let mut fg_color = None;
        let mut bg_color = None;

        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => attrs = TextAttributes::default(),
                1 | 5 => attrs.bold = true, // 5 = blink (deprecated, treated as bold)
                2 => attrs.hidden = true,
                3 => attrs.italic = true,
                4 => attrs.underline = true,
                7 => attrs.inverse = true,
                9 => attrs.strikethrough = true,
                21 | 24 => attrs.underline = false,
                22 => {
                    attrs.bold = false;
                    attrs.hidden = false;
                }
                23 => attrs.italic = false,
                27 => attrs.inverse = false,
                29 => attrs.strikethrough = false,
                30..=37 => fg_color = Some(Color::Named((p - 30) as u8)),
                38 => {
                    if i + 1 < params.len() {
                        if params[i + 1] == 5 && i + 2 < params.len() {
                            fg_color = Some(Color::Palette(params[i + 2] as u8));
                            i += 2;
                        } else if params[i + 1] == 2 && i + 4 < params.len() {
                            fg_color = Some(Color::Rgb {
                                r: params[i + 2] as u8,
                                g: params[i + 3] as u8,
                                b: params[i + 4] as u8,
                            });
                            i += 4;
                        }
                    }
                }
                39 => fg_color = Some(Color::Named(7)), // default foreground
                40..=47 => bg_color = Some(Color::Named((p - 40) as u8)),
                48 => {
                    if i + 1 < params.len() {
                        if params[i + 1] == 5 && i + 2 < params.len() {
                            bg_color = Some(Color::Palette(params[i + 2] as u8));
                            i += 2;
                        } else if params[i + 1] == 2 && i + 4 < params.len() {
                            bg_color = Some(Color::Rgb {
                                r: params[i + 2] as u8,
                                g: params[i + 3] as u8,
                                b: params[i + 4] as u8,
                            });
                            i += 4;
                        }
                    }
                }
                49 => bg_color = Some(Color::Named(0)), // default background
                _ => {}
            }
            i += 1;
        }

        actions.push(Action::SetAttributes(attrs));
        if let Some(color) = fg_color {
            actions.push(Action::SetForegroundColor(color));
        }
        if let Some(color) = bg_color {
            actions.push(Action::SetBackgroundColor(color));
        }
        if params.is_empty() || params == [0] {
            actions.push(Action::ResetAttributes);
        }
        actions
    }

    fn dispatch_decset(params: &[u16]) -> Vec<Action> {
        let mut actions = Vec::new();
        for &p in params {
            match p {
                1049 => actions.push(Action::EnterAlternateScreen),
                2004 => actions.push(Action::EnableBracketedPaste),
                22 => actions.push(Action::SetCursorShape(CursorShape::Block)),
                _ => {}
            }
        }
        actions
    }

    fn dispatch_decreset(params: &[u16]) -> Vec<Action> {
        let mut actions = Vec::new();
        for &p in params {
            match p {
                1049 => actions.push(Action::ExitAlternateScreen),
                2004 => actions.push(Action::DisableBracketedPaste),
                22 => actions.push(Action::SetCursorShape(CursorShape::Underline)),
                _ => {}
            }
        }
        actions
    }

    fn parse_osc(&mut self, byte: u8) -> Vec<Action> {
        if byte == 0x07 || byte == 0x9c {
            // ST (String Terminator) — BEL or ESC \
            // Parse the full numeric command (e.g., "22" from "22;2")
            let command_str: String = self
                .osc_payload
                .iter()
                .take_while(|&&b| b.is_ascii_digit())
                .map(|&b| b as char)
                .collect();
            let command: u8 = command_str.parse().unwrap_or(0);
            // payload is everything after the command; strip leading ';'
            let raw_payload: Vec<u8> = self.osc_payload[command_str.len()..].to_vec();
            self.osc_payload.clear();

            // Handle OSC 8 (hyperlinks) directly
            if command == 8 {
                // payload format: ";id;url" — first ';' separates cmd from id,
                // second ';' separates id from url
                if let Some(first_semi) = raw_payload.iter().position(|&b| b == b';') {
                    let after_id = &raw_payload[first_semi + 1..];
                    if let Some(second_semi) = after_id.iter().position(|&b| b == b';') {
                        let url = &after_id[second_semi + 1..];
                        if !url.is_empty() {
                            let url_str = String::from_utf8_lossy(url).to_string();
                            self.state = ParserState::Normal;
                            return vec![Action::HyperlinkStart(url_str)];
                        }
                    }
                }
                // No second ';' or empty URL = hyperlink end
                self.state = ParserState::Normal;
                return vec![Action::HyperlinkEnd];
            }

            // For other OSC commands, strip the leading ';'
            let payload = if raw_payload.first() == Some(&b';') {
                raw_payload[1..].to_vec()
            } else {
                raw_payload
            };

            self.state = ParserState::Normal;
            vec![Action::OscCommand(command, payload)]
        } else if byte == 0x1b {
            // Possible ESC \ (ST)
            self.state = ParserState::Escape;
            Vec::new()
        } else {
            self.osc_payload.push(byte);
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_printable() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"Hello");
        assert_eq!(
            actions,
            vec![
                Action::Write('H'),
                Action::Write('e'),
                Action::Write('l'),
                Action::Write('l'),
                Action::Write('o'),
            ]
        );
    }

    #[test]
    fn test_parse_bell() {
        let mut parser = Parser::new();
        let actions = parser.parse(0x07);
        assert_eq!(actions, vec![Action::Bell]);
    }

    #[test]
    fn test_parse_line_feed() {
        let mut parser = Parser::new();
        let actions = parser.parse(0x0a);
        assert_eq!(actions, vec![Action::LineFeed]);
    }

    #[test]
    fn test_parse_carriage_return() {
        let mut parser = Parser::new();
        let actions = parser.parse(0x0d);
        assert_eq!(actions, vec![Action::CarriageReturn]);
    }

    #[test]
    fn test_parse_sgr_reset() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[0m");
        assert!(actions.iter().any(|a| matches!(a, Action::ResetAttributes)));
    }

    #[test]
    fn test_parse_sgr_bold() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[1m");
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::SetAttributes(attrs) if attrs.bold)));
    }

    #[test]
    fn test_parse_sgr_foreground_color() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[31m");
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::SetForegroundColor(Color::Named(1)))));
    }

    #[test]
    fn test_parse_sgr_rgb_color() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[38;2;255;128;64m");
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::SetForegroundColor(Color::Rgb {
                r: 255,
                g: 128,
                b: 64
            })
        )));
    }

    #[test]
    fn test_parse_sgr_256_color() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[38;5;196m");
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::SetForegroundColor(Color::Palette(196)))));
    }

    #[test]
    fn test_parse_cursor_position() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[5;10H");
        assert_eq!(actions, vec![Action::CursorPosition(4, 9)]);
    }

    #[test]
    fn test_parse_cursor_up() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[3A");
        assert_eq!(actions, vec![Action::CursorUp(3)]);
    }

    #[test]
    fn test_parse_cursor_column() {
        let mut parser = Parser::new();
        // CHA must move to the column only (not reset the row).
        let actions = parser.parse_bytes(b"\x1b[10G");
        assert_eq!(actions, vec![Action::CursorToColumn(9)]);
    }

    #[test]
    fn test_parse_cursor_prev_next_line_reset_column() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[2F"),
            vec![Action::CursorUp(2), Action::CursorToColumn(0)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[2E"),
            vec![Action::CursorDown(2), Action::CursorToColumn(0)]
        );
    }

    #[test]
    fn test_parse_osc_title() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b]0;My Title\x07");
        assert_eq!(actions.len(), 1);
        if let Action::OscCommand(0, payload) = &actions[0] {
            assert_eq!(payload, b"My Title");
        } else {
            panic!("Expected OscCommand");
        }
    }

    #[test]
    fn test_parse_invalid_utf8_replaced() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(&[0xff, 0xfe, 0x41]); // invalid + 'A'
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0], Action::Write('\u{FFFD}'));
        assert_eq!(actions[1], Action::Write('\u{FFFD}'));
        assert_eq!(actions[2], Action::Write('A'));
    }

    #[test]
    fn test_parse_multibyte_utf8() {
        let mut parser = Parser::new();
        // 2-byte and 3-byte sequences must decode to single scalar values.
        assert_eq!(parser.parse_bytes("á".as_bytes()), vec![Action::Write('á')]);
        assert_eq!(
            parser.parse_bytes("\u{e0b0}".as_bytes()),
            vec![Action::Write('\u{e0b0}')]
        );
        // A valid sequence followed by ASCII.
        assert_eq!(
            parser.parse_bytes("é!".as_bytes()),
            vec![Action::Write('é'), Action::Write('!')]
        );
    }

    #[test]
    fn test_parse_incomplete_utf8_then_ascii() {
        let mut parser = Parser::new();
        // A truncated 2-byte sequence followed by ASCII: FFFD + the ASCII char.
        let actions = parser.parse_bytes(&[0xc3, 0x41]);
        assert_eq!(actions, vec![Action::Write('\u{FFFD}'), Action::Write('A')]);
    }

    #[test]
    fn test_parse_alternate_screen() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[?1049h");
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::EnterAlternateScreen)));
    }

    #[test]
    fn test_parse_bracketed_paste() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[?2004h");
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::EnableBracketedPaste)));
    }

    #[test]
    fn test_parse_hyperlink_osc8() {
        let mut parser = Parser::new();
        let input = b"\x1b]8;;https://example.com\x07Link\x1b]8;;\x07";
        let actions = parser.parse_bytes(input);
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::HyperlinkStart(_))));
        assert!(actions.iter().any(|a| matches!(a, Action::HyperlinkEnd)));
    }

    #[test]
    fn test_parser_reset() {
        let mut parser = Parser::new();
        parser.parse(0x1b); // ESC, enters Escape state
        assert!(matches!(parser.state, ParserState::Escape));
        parser.reset();
        assert!(matches!(parser.state, ParserState::Normal));
    }

    #[test]
    fn test_parse_default_params() {
        let mut parser = Parser::new();
        // CSI m with no params = reset
        let actions = parser.parse_bytes(b"\x1b[m");
        assert!(actions.iter().any(|a| matches!(a, Action::ResetAttributes)));
    }
}
