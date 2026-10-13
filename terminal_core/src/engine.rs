//! Terminal engine: ties parser, state, and grid together.
//!
//! The engine is the main entry point for feeding bytes into the terminal
//! and reading the resulting screen state.

use crate::buffer::GridBuffer;
use crate::cell::{Cell, CellStyle};
use crate::parser::{Action, Parser};
use crate::state::TerminalState;

/// The terminal engine: receives bytes, updates state, produces screen.
pub struct TerminalEngine {
    parser: Parser,
    state: TerminalState,
    buffer: GridBuffer,
}

impl TerminalEngine {
    /// Create a new engine with the given screen dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            parser: Parser::new(),
            state: TerminalState::default(),
            buffer: GridBuffer::new(rows, cols),
        }
    }

    /// Feed bytes into the terminal engine.
    /// Returns any actions that need to be handled by the GUI (e.g., clipboard).
    pub fn input(&mut self, bytes: &[u8]) -> Vec<Action> {
        let mut gui_actions = Vec::new();
        let actions = self.parser.parse_bytes(bytes);

        for action in actions {
            self.apply_action(action, &mut gui_actions);
        }

        gui_actions
    }

    /// Apply a parsed action to the terminal state and buffer.
    #[expect(clippy::too_many_lines, reason = "Terminal engine action handling")]
    fn apply_action(&mut self, action: Action, gui_actions: &mut Vec<Action>) {
        match action {
            Action::Write(c) => self.write_char(c),
            Action::CursorPosition(row, col) => self.state.cursor.move_to(row, col),
            Action::CursorUp(n) => {
                self.state.cursor.row = self.state.cursor.row.saturating_sub(n);
            }
            Action::CursorDown(n) => {
                self.state.cursor.row = self.state.cursor.row.saturating_add(n);
            }
            Action::CursorForward(n) => {
                self.state.cursor.col = self.state.cursor.col.saturating_add(n);
            }
            Action::CursorBackward(n) => {
                self.state.cursor.col = self.state.cursor.col.saturating_sub(n);
            }
            Action::CursorToColumn(col) => {
                let cols = self.buffer.active().grid.cols;
                self.state.cursor.col = col.min(cols.saturating_sub(1));
            }
            Action::CursorHome => {
                self.state.cursor.move_to(0, 0);
            }
            Action::EraseInLine => {
                let buf = self.buffer.active_mut();
                buf.clear_line();
            }
            Action::EraseInDisplay => {
                let buf = self.buffer.active_mut();
                buf.clear_screen();
            }
            Action::InsertLines(n) => {
                let buf = self.buffer.active_mut();
                buf.insert_lines(n);
            }
            Action::DeleteLines(n) => {
                let buf = self.buffer.active_mut();
                buf.delete_lines(n);
            }
            Action::LineFeed => {
                if self.buffer.is_alternate_screen_active() {
                    let buf = self.buffer.active_mut();
                    buf.cursor_row += 1;
                    if buf.cursor_row >= buf.grid.rows {
                        buf.scroll_up(1);
                        buf.cursor_row = buf.grid.rows - 1;
                    }
                } else {
                    self.buffer.main_line_feed();
                }
                // Sync state cursor with buffer
                let buf = self.buffer.active();
                self.state.cursor.row = buf.cursor().0;
                self.state.cursor.col = buf.cursor().1;
            }
            Action::CarriageReturn => {
                self.state.cursor.col = 0;
            }
            Action::Backspace => {
                self.state.cursor.col = self.state.cursor.col.saturating_sub(1);
            }
            Action::SetAttributes(attrs) => {
                self.state.attributes = attrs;
            }
            Action::SetForegroundColor(color) => {
                self.state.fg_color = color;
            }
            Action::SetBackgroundColor(color) => {
                self.state.bg_color = color;
            }
            Action::ResetAttributes => {
                self.state.reset_attributes();
            }
            Action::EnterAlternateScreen => {
                self.buffer.enter_alternate_screen();
            }
            Action::ExitAlternateScreen => {
                self.buffer.exit_alternate_screen();
            }
            Action::EnableBracketedPaste => {
                self.state.modes.bracketed_paste = true;
            }
            Action::DisableBracketedPaste => {
                self.state.modes.bracketed_paste = false;
            }
            Action::Bell => {
                gui_actions.push(Action::Bell);
            }
            Action::OscCommand(cmd, payload) => {
                TerminalEngine::handle_osc(cmd, &payload, gui_actions);
            }
            Action::SetCursorShape(shape) => {
                self.state.cursor.shape = shape;
            }
            Action::QueryForegroundColor => {
                // Query — pass to GUI to respond
                gui_actions.push(Action::QueryForegroundColor);
            }
            Action::QueryBackgroundColor => {
                // Query — pass to GUI to respond
                gui_actions.push(Action::QueryBackgroundColor);
            }
            Action::QueryTextColor => {
                // Query — pass to GUI to respond
                gui_actions.push(Action::QueryTextColor);
            }
            Action::SetForegroundColorFromOsc(color) => {
                // Color set from OSC 10 — pass to GUI for application
                gui_actions.push(Action::SetForegroundColorFromOsc(color));
            }
            Action::SetBackgroundColorFromOsc(color) => {
                // Color set from OSC 11 — pass to GUI for application
                gui_actions.push(Action::SetBackgroundColorFromOsc(color));
            }
            Action::SetTextColorFromOsc(color) => {
                // Color set from OSC 12 — pass to GUI for application
                gui_actions.push(Action::SetTextColorFromOsc(color));
            }
            Action::SetWindowTitle(title) => {
                gui_actions.push(Action::SetWindowTitle(title));
            }
            Action::SetIconName(title) => {
                gui_actions.push(Action::SetIconName(title));
            }
            Action::SetWindowIconAndTitle(title) => {
                gui_actions.push(Action::SetWindowIconAndTitle(title));
            }
            Action::MoveCursorTo(row, col) => {
                self.state.cursor.move_to(row, col);
            }
            Action::HyperlinkStart(url) => {
                gui_actions.push(Action::HyperlinkStart(url));
            }
            Action::HyperlinkEnd => {
                gui_actions.push(Action::HyperlinkEnd);
            }
            Action::Unknown => {}
        }
    }

    /// Write a character at the current cursor position.
    fn write_char(&mut self, c: char) {
        let row = self.state.cursor.row;
        let col = self.state.cursor.col;
        let style = CellStyle {
            fg: self.state.fg_color,
            bg: self.state.bg_color,
            bold: self.state.attributes.bold,
            italic: self.state.attributes.italic,
            underline: self.state.attributes.underline,
            inverse: self.state.attributes.inverse,
            hidden: self.state.attributes.hidden,
            strikethrough: self.state.attributes.strikethrough,
        };

        let buf = self.buffer.active_mut();
        buf.grid.set(row, col, Cell { c, style });

        // Advance cursor
        self.state.cursor.col += 1;
        if self.state.cursor.col >= buf.grid.cols {
            self.state.cursor.col = 0;
            self.state.cursor.row += 1;
            if self.state.cursor.row >= buf.grid.rows {
                if self.buffer.is_alternate_screen_active() {
                    self.buffer.alternate_line_feed();
                } else {
                    self.buffer.main_line_feed();
                }
                // Sync state cursor after scroll (buffer cursor is now at last row)
                let buf = self.buffer.active();
                self.state.cursor.row = buf.cursor().0;
                self.state.cursor.col = buf.cursor().1;
            }
        }
    }

    /// Handle OSC commands.
    fn handle_osc(cmd: u8, payload: &[u8], gui_actions: &mut Vec<Action>) {
        match cmd {
            0 => {
                // OSC 0: set icon name and window title
                let title = String::from_utf8_lossy(payload).to_string();
                if !title.is_empty() {
                    gui_actions.push(Action::SetWindowIconAndTitle(title));
                }
            }
            1 => {
                // OSC 1: set icon name
                let title = String::from_utf8_lossy(payload).to_string();
                if !title.is_empty() {
                    gui_actions.push(Action::SetIconName(title));
                }
            }
            2 => {
                // OSC 2: set window title
                let title = String::from_utf8_lossy(payload).to_string();
                if !title.is_empty() {
                    gui_actions.push(Action::SetWindowTitle(title));
                }
            }
            8 => {
                // OSC 8: hyperlink — format: "id;url"
                // Empty URL (no semi-colon) = end hyperlink
                if let Some(semi) = payload.iter().position(|&b| b == b';') {
                    let url = &payload[semi + 1..];
                    let url_str = String::from_utf8_lossy(url).to_string();
                    if !url_str.is_empty() {
                        gui_actions.push(Action::HyperlinkStart(url_str));
                    }
                } else {
                    // No semi-colon means hyperlink end
                    gui_actions.push(Action::HyperlinkEnd);
                }
            }
            10 => {
                // OSC 10: foreground color
                if payload.is_empty() {
                    // Query — GUI should respond
                    gui_actions.push(Action::QueryForegroundColor);
                } else {
                    // Set color
                    gui_actions.push(Action::SetForegroundColorFromOsc(
                        String::from_utf8_lossy(payload).to_string(),
                    ));
                }
            }
            11 => {
                // OSC 11: background color
                if payload.is_empty() {
                    // Query — GUI should respond
                    gui_actions.push(Action::QueryBackgroundColor);
                } else {
                    // Set color
                    gui_actions.push(Action::SetBackgroundColorFromOsc(
                        String::from_utf8_lossy(payload).to_string(),
                    ));
                }
            }
            12 => {
                // OSC 12: text color (same as foreground in most terminals)
                if payload.is_empty() {
                    // Query — GUI should respond
                    gui_actions.push(Action::QueryTextColor);
                } else {
                    // Set color
                    gui_actions.push(Action::SetTextColorFromOsc(
                        String::from_utf8_lossy(payload).to_string(),
                    ));
                }
            }
            200 => {
                // Bracketed paste start
                let text = String::from_utf8_lossy(payload).to_string();
                if !text.is_empty() {
                    gui_actions.push(Action::Write(text.chars().next().unwrap_or('\0')));
                }
            }
            22 => {
                // OSC 22: cursor shape
                // 0 = default (block), 1 = beam, 2 = block, 3 = underline
                let shape = match payload.first().copied() {
                    Some(b'0' | b'2') => crate::state::CursorShape::Block,
                    Some(b'1') => crate::state::CursorShape::Beam,
                    Some(b'3') => crate::state::CursorShape::Underline,
                    _ => crate::state::CursorShape::default(),
                };
                gui_actions.push(Action::SetCursorShape(shape));
            }
            _ => {}
        }
    }

    /// Get a reference to the current buffer.
    pub fn buffer(&self) -> &GridBuffer {
        &self.buffer
    }

    /// Get a reference to the current state.
    pub fn state(&self) -> &TerminalState {
        &self.state
    }

    /// Get a mutable reference to the current state.
    pub fn state_mut(&mut self) -> &mut TerminalState {
        &mut self.state
    }

    /// Get screen dimensions.
    pub fn dimensions(&self) -> (usize, usize) {
        let buf = self.buffer.active();
        (buf.grid.rows, buf.grid.cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Color;

    #[test]
    fn test_write_text() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"Hello");
        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, 'H');
        assert_eq!(buf.grid.get(0, 1).c, 'e');
        assert_eq!(buf.grid.get(0, 2).c, 'l');
        assert_eq!(buf.grid.get(0, 3).c, 'l');
        assert_eq!(buf.grid.get(0, 4).c, 'o');
    }

    #[test]
    fn test_sgr_color() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"\x1b[31mRed");
        assert_eq!(
            engine.buffer().active().grid.get(0, 0).style.fg,
            Color::Named(1)
        );
        assert_eq!(
            engine.buffer().active().grid.get(0, 1).style.fg,
            Color::Named(1)
        );
        assert_eq!(
            engine.buffer().active().grid.get(0, 2).style.fg,
            Color::Named(1)
        );
    }

    #[test]
    fn test_cursor_move() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"\x1b[5;10H");
        assert_eq!(engine.state().cursor.row, 4);
        assert_eq!(engine.state().cursor.col, 9);
    }

    #[test]
    fn test_cha_keeps_row() {
        let mut engine = TerminalEngine::new(24, 80);
        // Move to row 5, then CHA to column 11 (0-indexed 10).
        engine.input(b"\x1b[6;1H\x1b[11G");
        assert_eq!(engine.state().cursor.row, 5);
        assert_eq!(engine.state().cursor.col, 10);
    }

    #[test]
    fn test_sgr_reset_keeps_cursor() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"AB\x1b[0mC");
        // SGR reset (ESC [ 0 m) must not move the cursor.
        assert_eq!(engine.state().cursor.col, 3);
        assert_eq!(engine.state().fg_color, Color::Named(7));
    }

    #[test]
    fn test_line_feed() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"Line1\nLine2");
        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, 'L');
        assert_eq!(buf.grid.get(1, 0).c, 'L');
    }

    #[test]
    fn test_wrap_around() {
        let mut engine = TerminalEngine::new(1, 5);
        engine.input(b"ABCDE");
        // After writing 5 chars in a 5-col grid, cursor wraps to col 0
        assert_eq!(engine.state().cursor.col, 0);
        assert_eq!(engine.state().cursor.row, 0); // scrolled to first row
        engine.input(b"F");
        let buf = engine.buffer().active();
        assert_eq!(buf.grid.rows, 1);
    }

    #[test]
    fn test_alternate_screen() {
        let mut engine = TerminalEngine::new(24, 80);
        // Write on main
        engine.input(b"Main");
        // Enter alternate
        engine.input(b"\x1b[?1049h");
        assert!(engine.buffer().is_alternate_screen_active());
        // Alternate is empty
        assert_eq!(engine.buffer().active().grid.get(0, 0).c, ' ');
        // Exit alternate
        engine.input(b"\x1b[?1049l");
        assert!(!engine.buffer().is_alternate_screen_active());
        // Main still has "Main"
        assert_eq!(engine.buffer().active().grid.get(0, 0).c, 'M');
    }

    #[test]
    fn test_bell_action() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x07");
        assert!(gui_actions.iter().any(|a| matches!(a, Action::Bell)));
    }

    #[test]
    fn test_invalid_bytes_replaced() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(&[0xff, 0x41]); // invalid + 'A'
        assert_eq!(engine.buffer().active().grid.get(0, 0).c, '\u{FFFD}');
        assert_eq!(engine.buffer().active().grid.get(0, 1).c, 'A');
    }

    #[test]
    fn test_osc_title() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]0;Test Title\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetWindowIconAndTitle(t) if t == "Test Title")));
    }

    #[test]
    fn test_osc_10_query_fg() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]10;\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::QueryForegroundColor)));
    }

    #[test]
    fn test_osc_10_set_fg() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]10;#ff0000\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetForegroundColorFromOsc(c) if c == "#ff0000")));
    }

    #[test]
    fn test_osc_11_query_bg() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]11;\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::QueryBackgroundColor)));
    }

    #[test]
    fn test_osc_11_set_bg() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]11;#00ff00\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetBackgroundColorFromOsc(c) if c == "#00ff00")));
    }

    #[test]
    fn test_osc_12_query_text() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]12;\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::QueryTextColor)));
    }

    #[test]
    fn test_osc_12_set_text() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]12;#0000ff\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetTextColorFromOsc(c) if c == "#0000ff")));
    }

    #[test]
    fn test_osc_0_set_icon_and_title() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]0;My Icon And Title\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetWindowIconAndTitle(t) if t == "My Icon And Title")));
    }

    #[test]
    fn test_osc_1_set_icon() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]1;My Icon\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetIconName(t) if t == "My Icon")));
    }

    #[test]
    fn test_osc_2_set_title() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]2;My Title\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetWindowTitle(t) if t == "My Title")));
    }

    #[test]
    fn test_osc_22_cursor_block() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]22;2\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetCursorShape(crate::state::CursorShape::Block))));
    }

    #[test]
    fn test_osc_22_cursor_beam() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]22;1\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetCursorShape(crate::state::CursorShape::Beam))));
    }

    #[test]
    fn test_osc_22_cursor_underline() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]22;3\x07");
        assert!(gui_actions.iter().any(|a| matches!(
            a,
            Action::SetCursorShape(crate::state::CursorShape::Underline)
        )));
    }

    #[test]
    fn test_osc_22_cursor_default() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]22;0\x07");
        assert!(gui_actions
            .iter()
            .any(|a| matches!(a, Action::SetCursorShape(crate::state::CursorShape::Block))));
    }

    #[test]
    fn test_osc_22_cursor_invalid_shape() {
        let mut engine = TerminalEngine::new(24, 80);
        let gui_actions = engine.input(b"\x1b]22;9\x07");
        // Invalid shape should default to default shape
        assert!(gui_actions.iter().any(
            |a| matches!(a, Action::SetCursorShape(s) if s == &crate::state::CursorShape::default())
        ));
    }

    #[test]
    fn test_dimensions() {
        let engine = TerminalEngine::new(24, 80);
        assert_eq!(engine.dimensions(), (24, 80));
    }
}
