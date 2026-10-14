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
use crate::state::CursorShape;

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
    /// Move cursor to the given row, keeping the current column (VPA).
    CursorToRow(usize),
    /// Send cursor to beginning of line.
    CursorHome,
    /// Erase part of the current line (EL).
    ///
    /// The value is the ECMA-48 mode: `0` = cursor→end, `1` = start→cursor,
    /// `2` = whole line.
    EraseInLine(u8),
    /// Erase part of the display (ED).
    ///
    /// The value is the ECMA-48 mode: `0` = cursor→end, `1` = start→cursor,
    /// `2` = whole screen.
    EraseInDisplay(u8),
    /// Erase `n` characters starting at the cursor, without moving it (ECH).
    EraseChars(usize),
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
    ///
    /// Carries a single SGR parameter, which the engine applies to the
    /// attributes it already has: `ESC [ 1 m` followed by `ESC [ 31 m` must
    /// keep the text bold, so a colour sequence may not replace the whole
    /// attribute set.
    SetTextAttribute(u16),
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
    /// Set cursor blinking (DECSCUSR) — RF-49.
    SetCursorBlink(bool),
    /// Show or hide the cursor (DECTCEM, `ESC [ ? 25 h/l`) — RF-48.
    SetCursorVisible(bool),
    /// Start or end synchronized output (`ESC [ ? 2026 h/l`) — RF-47.
    SetSynchronizedOutput(bool),
    /// Save the cursor and attributes (SCP, `ESC [ s` / `ESC 7`) — RF-50.
    SaveCursor,
    /// Restore the cursor and attributes (RCP, `ESC [ u` / `ESC 8`) — RF-50.
    RestoreCursor,
    /// Move cursor to position (OSC 22).
    MoveCursorTo(usize, usize),
    /// Query foreground color (OSC 10) — GUI responds with current color.
    QueryForegroundColor,
    /// Query background color (OSC 11) — GUI responds with current color.
    QueryBackgroundColor,
    /// Query text color (OSC 12) — GUI responds with current color.
    QueryTextColor,
    /// Report cursor position (DSR, `ESC [ 6 n`) — RF-51.
    QueryCursorPosition,
    /// Report device attributes (DA, `ESC [ c`) — RF-51.
    QueryDeviceAttributes,
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
    /// Received ESC followed by an intermediate byte (e.g. `ESC ( `), waiting
    /// for the final byte of the sequence.
    EscapeIntermediate,
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
    csi_private: Option<u8>,
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
            csi_private: None,
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
            ParserState::EscapeIntermediate => {
                // Consume the final byte of `ESC <intermediate> <final>`
                // (charset and similar designators) and ignore the sequence.
                self.state = ParserState::Normal;
                Vec::new()
            }
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
        self.csi_private = None;
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
                self.csi_private = None;
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
            b'7' => vec![Action::SaveCursor],
            b'8' => vec![Action::RestoreCursor],
            0x20..=0x2f => {
                // Intermediate byte: `ESC <intermediate> <final>` (character
                // set designators like `ESC ( B`, `ESC ) 0`, `ESC # 8`, …).
                // Consume the final byte so it is not mistaken for text.
                self.state = ParserState::EscapeIntermediate;
                Vec::new()
            }
            _ => {
                // Unknown ESC sequence
                Vec::new()
            }
        }
    }

    fn parse_csi(&mut self, byte: u8) -> Vec<Action> {
        if (0x3c..=0x3f).contains(&byte)
            && self.csi_params.is_empty()
            && self.csi_intermediates.is_empty()
            && self.csi_private.is_none()
        {
            // Private parameter marker (`?`, `>`, `<`, `=`) — only valid as the
            // first byte after `ESC [`. Remember it so the final byte is not
            // dispatched as if it were the unprefixed command: `ESC [ > 4 ; 1 m`
            // (XTMODKEYS) must not be read as SGR `4;1` (underline + bold).
            self.csi_private = Some(byte);
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
            self.csi_private = None;
            self.state = ParserState::Normal;
            actions
        } else {
            // Abort CSI
            self.csi_params.clear();
            self.csi_intermediates.clear();
            self.csi_private = None;
            Vec::new()
        }
    }

    fn dispatch_csi(&self, final_byte: u8) -> Vec<Action> {
        // A private parameter marker selects a different command set. Only the
        // DEC private modes (`?` + `h`/`l`) map to DECSET/DECRESET here; the
        // rest (`ESC [ > … m`, `ESC [ > … q`, `ESC [ ? … u`, …) belong to
        // vendor extensions this parser does not implement and must not be
        // dispatched under the unprefixed final byte.
        if let Some(marker) = self.csi_private {
            if self.csi_intermediates.is_empty() && marker == b'?' {
                return match final_byte {
                    b'h' => Parser::dispatch_decset(&self.csi_params),
                    b'l' => Parser::dispatch_decreset(&self.csi_params),
                    _ => Vec::new(),
                };
            }
            return Vec::new();
        }

        if !self.csi_intermediates.is_empty() {
            // DECSCUSR (`ESC [ Ps SP q`) is the only sequence with an
            // intermediate byte this parser understands.
            if self.csi_intermediates == [b' '] && final_byte == b'q' {
                let ps = self.csi_params.first().copied().unwrap_or(0);
                return Parser::dispatch_decscusr(ps);
            }
            return Vec::new();
        }

        // Keep the parameter list empty when the sequence has none, so each
        // command's own default applies. ECMA-48 gives CUU/CUD/CUF/CUB (and
        // IL/DL) a default parameter of 1, while ED/EL default to 0. Forcing
        // `vec![0]` here would turn a bare `ESC [ C` into a no-op.
        let params = self.csi_params.clone();

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
            // VPA (Vertical Position Absolute): move to row only, keep column
            b'd' => {
                let row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                vec![Action::CursorToRow(row)]
            }
            // ECH (Erase Character): blank n cells without moving the cursor
            b'X' => vec![Action::EraseChars(
                params.first().copied().unwrap_or(1) as usize
            )],
            // ED (Erase in Display)
            b'J' => match params.first().copied().unwrap_or(0) {
                mode @ 0..=2 => vec![Action::EraseInDisplay(mode as u8)],
                _ => Vec::new(),
            },
            // EL (Erase in Line)
            b'K' => match params.first().copied().unwrap_or(0) {
                mode @ 0..=2 => vec![Action::EraseInLine(mode as u8)],
                _ => Vec::new(),
            },
            // IL (Insert Line)
            b'L' => vec![Action::InsertLines(
                params.first().copied().unwrap_or(1) as usize
            )],
            // SCP (Save Cursor Position)
            b's' => vec![Action::SaveCursor],
            // RCP (Restore Cursor Position)
            b'u' => vec![Action::RestoreCursor],
            // DSR (Device Status Report): `ESC [ 6 n` asks for the cursor
            // position; other parameters are ignored (RF-51).
            b'n' => match params.first().copied() {
                Some(6) => vec![Action::QueryCursorPosition],
                _ => Vec::new(),
            },
            // DA (Device Attributes): `ESC [ c` (RF-51).
            b'c' => vec![Action::QueryDeviceAttributes],
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

    /// Dispatch an SGR (Select Graphic Rendition) sequence.
    ///
    /// Each parameter is emitted as its own action so the engine applies it to
    /// the attributes and colours it already has. Emitting the whole attribute
    /// set here would make any colour sequence (`ESC [ 38;5;2 m`) clear the
    /// bold or italic set by an earlier one.
    fn dispatch_sgr(params: &[u16]) -> Vec<Action> {
        // A bare `ESC [ m` is `ESC [ 0 m`.
        if params.is_empty() {
            return vec![Action::ResetAttributes];
        }

        let mut actions = Vec::new();
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                // 0 resets attributes *and* colours, wherever it appears.
                0 => actions.push(Action::ResetAttributes),
                // Attribute codes: bold, italic, underline, inverse, …
                1..=9 | 21..=29 => actions.push(Action::SetTextAttribute(p)),
                30..=37 => actions.push(Action::SetForegroundColor(Color::Named((p - 30) as u8))),
                38 => {
                    if let Some((color, extra)) = Parser::extended_color(params, i) {
                        actions.push(Action::SetForegroundColor(color));
                        i += extra;
                    }
                }
                39 => actions.push(Action::SetForegroundColor(Color::Named(7))), // default foreground
                40..=47 => actions.push(Action::SetBackgroundColor(Color::Named((p - 40) as u8))),
                48 => {
                    if let Some((color, extra)) = Parser::extended_color(params, i) {
                        actions.push(Action::SetBackgroundColor(color));
                        i += extra;
                    }
                }
                49 => actions.push(Action::SetBackgroundColor(Color::Named(0))), // default background
                _ => {}
            }
            i += 1;
        }
        actions
    }

    /// Read an extended colour (`38`/`48`) starting at `start`.
    ///
    /// Returns the colour and how many extra parameters it consumed
    /// (`…;5;N` takes two, `…;2;R;G;B` takes four).
    fn extended_color(params: &[u16], start: usize) -> Option<(Color, usize)> {
        match *params.get(start + 1)? {
            // 256-colour palette.
            5 => {
                let index = *params.get(start + 2)?;
                Some((Color::Palette(index as u8), 2))
            }
            // 24-bit RGB.
            2 => {
                let r = *params.get(start + 2)?;
                let g = *params.get(start + 3)?;
                let b = *params.get(start + 4)?;
                Some((
                    Color::Rgb {
                        r: r as u8,
                        g: g as u8,
                        b: b as u8,
                    },
                    4,
                ))
            }
            _ => None,
        }
    }

    /// Map a DECSCUSR parameter to a cursor shape and blink flag (RF-49).
    ///
    /// `0`/`1` = blinking block, `2` = steady block, `3` = blinking underline,
    /// `4` = steady underline, `5` = blinking bar, `6` = steady bar.
    fn dispatch_decscusr(ps: u16) -> Vec<Action> {
        let (shape, blink) = match ps {
            2 => (CursorShape::Block, false),
            3 => (CursorShape::Underline, true),
            4 => (CursorShape::Underline, false),
            5 => (CursorShape::Beam, true),
            6 => (CursorShape::Beam, false),
            _ => (CursorShape::Block, true),
        };
        vec![Action::SetCursorShape(shape), Action::SetCursorBlink(blink)]
    }

    fn dispatch_decset(params: &[u16]) -> Vec<Action> {
        let mut actions = Vec::new();
        for &p in params {
            match p {
                1049 => actions.push(Action::EnterAlternateScreen),
                2004 => actions.push(Action::EnableBracketedPaste),
                22 => actions.push(Action::SetCursorShape(CursorShape::Block)),
                25 => actions.push(Action::SetCursorVisible(true)),
                2026 => actions.push(Action::SetSynchronizedOutput(true)),
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
                25 => actions.push(Action::SetCursorVisible(false)),
                2026 => actions.push(Action::SetSynchronizedOutput(false)),
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
        assert_eq!(actions, vec![Action::SetTextAttribute(1)]);
    }

    #[test]
    fn test_parse_sgr_attributes_are_individual_actions() {
        // Each attribute travels on its own so the engine can apply it to the
        // attributes it already has.
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[3m"),
            vec![Action::SetTextAttribute(3)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[4m"),
            vec![Action::SetTextAttribute(4)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[7m"),
            vec![Action::SetTextAttribute(7)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[22m"),
            vec![Action::SetTextAttribute(22)]
        );
    }

    #[test]
    fn test_parse_sgr_colour_does_not_touch_attributes() {
        // Regression: a colour sequence used to emit a full attribute set,
        // which wiped bold/italic set by an earlier sequence.
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[38;5;2;49m");
        assert_eq!(
            actions,
            vec![
                Action::SetForegroundColor(Color::Palette(2)),
                Action::SetBackgroundColor(Color::Named(0)),
            ]
        );
    }

    #[test]
    fn test_parse_sgr_reset_in_the_middle() {
        // SGR 0 resets attributes and colours wherever it appears.
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[0;1m");
        assert_eq!(
            actions,
            vec![Action::ResetAttributes, Action::SetTextAttribute(1)]
        );
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
    fn test_parse_vertical_position_absolute() {
        // VPA moves to a row and keeps the column; htop steps down the screen
        // with `CR` + `ESC [ <row> d` instead of a full CUP.
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[10d"),
            vec![Action::CursorToRow(9)]
        );
        assert_eq!(parser.parse_bytes(b"\x1b[2d"), vec![Action::CursorToRow(1)]);
        // A bare `ESC [ d` defaults to the first row.
        assert_eq!(parser.parse_bytes(b"\x1b[d"), vec![Action::CursorToRow(0)]);
    }

    #[test]
    fn test_parse_erase_character() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[24X"),
            vec![Action::EraseChars(24)]
        );
        // A bare `ESC [ X` erases a single character.
        assert_eq!(parser.parse_bytes(b"\x1b[X"), vec![Action::EraseChars(1)]);
        // An explicit zero erases nothing.
        assert_eq!(parser.parse_bytes(b"\x1b[0X"), vec![Action::EraseChars(0)]);
    }

    #[test]
    fn test_parse_cursor_up() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[3A");
        assert_eq!(actions, vec![Action::CursorUp(3)]);
    }

    #[test]
    fn test_parse_cursor_forward_default_one() {
        // ECMA-48: a bare `ESC [ C` (CUF) moves one column, not zero. Readline
        // relies on this to reposition the cursor after each echo.
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[C"),
            vec![Action::CursorForward(1)]
        );
    }

    #[test]
    fn test_parse_cursor_default_one_for_abcd() {
        let mut parser = Parser::new();
        assert_eq!(parser.parse_bytes(b"\x1b[A"), vec![Action::CursorUp(1)]);
        assert_eq!(parser.parse_bytes(b"\x1b[B"), vec![Action::CursorDown(1)]);
        assert_eq!(
            parser.parse_bytes(b"\x1b[C"),
            vec![Action::CursorForward(1)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[D"),
            vec![Action::CursorBackward(1)]
        );
    }

    #[test]
    fn test_parse_insert_delete_line_default_one() {
        let mut parser = Parser::new();
        assert_eq!(parser.parse_bytes(b"\x1b[L"), vec![Action::InsertLines(1)]);
        assert_eq!(parser.parse_bytes(b"\x1b[M"), vec![Action::DeleteLines(1)]);
    }

    #[test]
    fn test_parse_erase_defaults_stay_zero() {
        // ED/EL keep a default of 0 (erase from cursor), unlike cursor moves.
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[J"),
            vec![Action::EraseInDisplay(0)]
        );
        assert_eq!(parser.parse_bytes(b"\x1b[K"), vec![Action::EraseInLine(0)]);
    }

    #[test]
    fn test_parse_erase_modes_are_preserved() {
        // Full-screen apps (top, vim) rely on the exact erase mode.
        let mut parser = Parser::new();
        assert_eq!(parser.parse_bytes(b"\x1b[0K"), vec![Action::EraseInLine(0)]);
        assert_eq!(parser.parse_bytes(b"\x1b[1K"), vec![Action::EraseInLine(1)]);
        assert_eq!(parser.parse_bytes(b"\x1b[2K"), vec![Action::EraseInLine(2)]);
        assert_eq!(
            parser.parse_bytes(b"\x1b[0J"),
            vec![Action::EraseInDisplay(0)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[1J"),
            vec![Action::EraseInDisplay(1)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[2J"),
            vec![Action::EraseInDisplay(2)]
        );
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
    fn test_private_xtmodkeys_is_not_sgr() {
        // Regression: `ESC [ > 4 ; 1 m` (XTMODKEYS) was dispatched as SGR
        // `4;1` (underline + bold), painting a rule under every cell of a TUI's
        // screen-wide fill (opencode).
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[>4;1m");
        assert_eq!(actions, Vec::new());
    }

    #[test]
    fn test_unprefixed_sgr_still_applies() {
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[4;1m");
        assert_eq!(
            actions,
            vec![Action::SetTextAttribute(4), Action::SetTextAttribute(1)]
        );
    }

    #[test]
    fn test_private_marker_does_not_leak_into_text() {
        // The bytes of a private CSI must be consumed, not rendered as text.
        let mut parser = Parser::new();
        let actions = parser.parse_bytes(b"\x1b[>0qA");
        assert_eq!(actions, vec![Action::Write('A')]);
    }

    #[test]
    fn test_parse_cursor_visibility() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[?25l"),
            vec![Action::SetCursorVisible(false)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[?25h"),
            vec![Action::SetCursorVisible(true)]
        );
    }

    #[test]
    fn test_parse_synchronized_output() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[?2026h"),
            vec![Action::SetSynchronizedOutput(true)]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[?2026l"),
            vec![Action::SetSynchronizedOutput(false)]
        );
    }

    #[test]
    fn test_parse_save_and_restore_cursor() {
        let mut parser = Parser::new();
        assert_eq!(parser.parse_bytes(b"\x1b[s"), vec![Action::SaveCursor]);
        assert_eq!(parser.parse_bytes(b"\x1b[u"), vec![Action::RestoreCursor]);
        // The DEC forms are equivalent.
        assert_eq!(parser.parse_bytes(b"\x1b7"), vec![Action::SaveCursor]);
        assert_eq!(parser.parse_bytes(b"\x1b8"), vec![Action::RestoreCursor]);
    }

    #[test]
    fn test_private_query_keyboard_is_not_restore_cursor() {
        // `ESC [ ? u` queries the keyboard protocol; it must not be read as RCP.
        let mut parser = Parser::new();
        assert_eq!(parser.parse_bytes(b"\x1b[?u"), Vec::new());
    }

    #[test]
    fn test_parse_decscusr() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[1 q"),
            vec![
                Action::SetCursorShape(CursorShape::Block),
                Action::SetCursorBlink(true)
            ]
        );
        assert_eq!(
            parser.parse_bytes(b"\x1b[6 q"),
            vec![
                Action::SetCursorShape(CursorShape::Beam),
                Action::SetCursorBlink(false)
            ]
        );
    }

    #[test]
    fn test_parse_cursor_position_request() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[6n"),
            vec![Action::QueryCursorPosition]
        );
        // Other DSR parameters are not answered.
        assert_eq!(parser.parse_bytes(b"\x1b[5n"), Vec::new());
    }

    #[test]
    fn test_parse_device_attributes_request() {
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"\x1b[c"),
            vec![Action::QueryDeviceAttributes]
        );
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
    fn test_parse_charset_designator_is_ignored() {
        // `ESC ( B` selects the ASCII charset; it must not leak the 'B' as text.
        let mut parser = Parser::new();
        assert_eq!(
            parser.parse_bytes(b"A\x1b(BB"),
            vec![Action::Write('A'), Action::Write('B')]
        );
        // `ESC ) 0` (G1 designator) is ignored too.
        assert_eq!(parser.parse_bytes(b"\x1b)0Z"), vec![Action::Write('Z')]);
        // `ESC # 8` (DECALN) is ignored.
        assert_eq!(parser.parse_bytes(b"\x1b#8X"), vec![Action::Write('X')]);
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
