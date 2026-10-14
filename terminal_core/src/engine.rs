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
    /// Set when a character is written in the last column: the cursor only
    /// wraps on the next printable character (DECAWM deferred wrap).
    wrap_pending: bool,
}

impl TerminalEngine {
    /// Create a new engine with the given screen dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            parser: Parser::new(),
            state: TerminalState::default(),
            buffer: GridBuffer::new(rows, cols),
            wrap_pending: false,
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
        // `state.cursor` is the engine's source of truth, but buffer-level
        // operations (`clear_line`, `insert_lines`, `delete_lines`, …) read the
        // `ScreenBuffer`'s own cursor fields. Mirror the cursor first so those
        // operations act on the current position instead of a stale one.
        self.sync_cursor_to_buffer();

        match action {
            Action::Write(c) => self.write_char(c),
            Action::CursorPosition(row, col) | Action::MoveCursorTo(row, col) => {
                self.state.cursor.move_to(row, col);
                self.wrap_pending = false;
            }
            Action::CursorUp(n) => {
                self.state.cursor.row = self.state.cursor.row.saturating_sub(n);
                self.wrap_pending = false;
            }
            Action::CursorDown(n) => {
                self.state.cursor.row = self.state.cursor.row.saturating_add(n);
                self.wrap_pending = false;
            }
            Action::CursorForward(n) => {
                self.state.cursor.col = self.state.cursor.col.saturating_add(n);
                self.wrap_pending = false;
            }
            Action::CursorBackward(n) => {
                self.state.cursor.col = self.state.cursor.col.saturating_sub(n);
                self.wrap_pending = false;
            }
            Action::CursorToColumn(col) => {
                let cols = self.buffer.active().grid.cols;
                self.state.cursor.col = col.min(cols.saturating_sub(1));
                self.wrap_pending = false;
            }
            Action::CursorToRow(row) => {
                let rows = self.buffer.active().grid.rows;
                self.state.cursor.row = row.min(rows.saturating_sub(1));
                self.wrap_pending = false;
            }
            Action::CursorHome => {
                self.state.cursor.move_to(0, 0);
                self.wrap_pending = false;
            }
            Action::EraseInLine(mode) => {
                let (row, col) = (self.state.cursor.row, self.state.cursor.col);
                let buf = self.buffer.active_mut();
                buf.erase_in_line(row, col, mode);
            }
            Action::EraseInDisplay(mode) => {
                let (row, col) = (self.state.cursor.row, self.state.cursor.col);
                let buf = self.buffer.active_mut();
                buf.erase_in_display(row, col, mode);
            }
            Action::EraseChars(n) => {
                let (row, col) = (self.state.cursor.row, self.state.cursor.col);
                let buf = self.buffer.active_mut();
                buf.erase_chars(row, col, n);
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
                    self.buffer.alternate_line_feed();
                } else {
                    self.buffer.main_line_feed();
                }
                // A line feed moves down one row (scrolling at the bottom) and
                // resets the column, which is the meaning `\n` has here.
                self.state.cursor.row = self.buffer.active().cursor().0;
                self.state.cursor.col = 0;
                self.wrap_pending = false;
            }
            Action::CarriageReturn => {
                self.state.cursor.col = 0;
                self.wrap_pending = false;
            }
            Action::Backspace => {
                self.state.cursor.col = self.state.cursor.col.saturating_sub(1);
                self.wrap_pending = false;
            }
            Action::SetTextAttribute(param) => {
                self.state.attributes.apply_sgr(param);
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
            Action::HyperlinkStart(url) => {
                gui_actions.push(Action::HyperlinkStart(url));
            }
            Action::HyperlinkEnd => {
                gui_actions.push(Action::HyperlinkEnd);
            }
            Action::Unknown => {}
        }
    }

    /// Mirror [`TerminalState::cursor`] into the active screen buffer.
    ///
    /// [`TerminalState::cursor`] is the single source of truth for the engine,
    /// while buffer operations such as `clear_line` or `insert_lines` use the
    /// `ScreenBuffer`'s cursor fields. Keeping both in sync avoids acting on a
    /// stale position (for example `ESC [ K` erasing from column 0).
    fn sync_cursor_to_buffer(&mut self) {
        let (row, col) = (self.state.cursor.row, self.state.cursor.col);
        let buf = self.buffer.active_mut();
        buf.cursor_row = row;
        buf.cursor_col = col;
    }

    /// Write a character at the current cursor position.
    fn write_char(&mut self, c: char) {
        let cols = self.buffer.active().grid.cols;
        let rows = self.buffer.active().grid.rows;

        // Deferred wrap (DECAWM): writing the last column does not move to the
        // next row by itself; the wrap happens when the next character is
        // written. Without this, a line exactly `cols` wide consumes two rows.
        if self.wrap_pending {
            self.wrap_pending = false;
            self.state.cursor.col = 0;
            self.state.cursor.row += 1;
            if self.state.cursor.row >= rows {
                if self.buffer.is_alternate_screen_active() {
                    self.buffer.alternate_line_feed();
                } else {
                    self.buffer.main_line_feed();
                }
                self.state.cursor.row = self.buffer.active().cursor().0;
            }
        }

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

        // Advance the column, or arm the deferred wrap at the last column.
        if self.state.cursor.col + 1 >= cols {
            self.wrap_pending = true;
        } else {
            self.state.cursor.col += 1;
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

    /// Resize the screen to the given dimensions.
    ///
    /// Content is preserved where it fits; the cursor is clamped to the new
    /// bounds.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        if rows == 0 || cols == 0 {
            return;
        }
        if self.dimensions() == (rows, cols) {
            return;
        }
        self.buffer.resize(rows, cols);
        self.state.cursor.row = self.state.cursor.row.min(rows - 1);
        self.state.cursor.col = self.state.cursor.col.min(cols - 1);
        self.sync_cursor_to_buffer();
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
    fn test_erase_in_line_modes() {
        // 2K erases the whole line, wherever the cursor is.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"abcdef\x1b[4G\x1b[2K");
        for col in 0..6 {
            assert_eq!(engine.buffer().active().grid.get(0, col).c, ' ');
        }

        // 1K erases from the start up to (and including) the cursor.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"abcdef\x1b[4G\x1b[1K");
        let buf = engine.buffer().active();
        for col in 0..4 {
            assert_eq!(buf.grid.get(0, col).c, ' ');
        }
        assert_eq!(buf.grid.get(0, 4).c, 'e');

        // 0K erases from the cursor to the end.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"abcdef\x1b[4G\x1b[0K");
        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 2).c, 'c');
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(buf.grid.get(0, 5).c, ' ');
    }

    #[test]
    fn test_erase_in_display_modes() {
        // 0J erases from the cursor down, keeping earlier lines intact.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"one\r\ntwo\r\nthree\x1b[2;1H\x1b[0J");
        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, 'o');
        assert_eq!(buf.grid.get(1, 0).c, ' ');
        assert_eq!(buf.grid.get(2, 0).c, ' ');

        // 2J erases the whole screen.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"one\r\ntwo\x1b[2J");
        assert_eq!(engine.buffer().active().grid.get(0, 0).c, ' ');
        assert_eq!(engine.buffer().active().grid.get(1, 0).c, ' ');
    }

    #[test]
    fn test_readline_echo_does_not_overwrite() {
        // Regression: interactive shells (readline) echo each keystroke as
        // `<char> CR <prompt> ESC[C*n`, moving right with a bare `ESC [ C`.
        // The cursor must advance so previously typed characters survive.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"> ");
        engine.input(b"h\r> \x1b[C");
        engine.input(b"e\r> \x1b[C\x1b[C");

        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, '>');
        assert_eq!(buf.grid.get(0, 1).c, ' ');
        assert_eq!(buf.grid.get(0, 2).c, 'h');
        assert_eq!(buf.grid.get(0, 3).c, 'e');
        assert_eq!(engine.state().cursor.col, 4);
    }

    #[test]
    fn test_erase_in_line_starts_at_cursor() {
        // Regression: `ESC [ K` (EL) must erase from the cursor to the end of
        // the line, not from column 0. The engine cursor lives in
        // `state.cursor`, so the buffer cursor has to be kept in sync.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"abcdef");
        engine.input(b"\x1b[3D"); // cursor back to column 3
        engine.input(b"\x1b[K"); // erase 3..end

        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, 'a');
        assert_eq!(buf.grid.get(0, 1).c, 'b');
        assert_eq!(buf.grid.get(0, 2).c, 'c');
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(buf.grid.get(0, 5).c, ' ');
    }

    #[test]
    fn test_readline_backspace_erases_only_last_char() {
        // Readline deletes a character with `BS ESC [ K CR <prompt> ESC [ C*n`.
        // Only the character under the cursor may be erased.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"> ");
        engine.input(b"a\r> \x1b[C");
        engine.input(b"b\r> \x1b[C\x1b[C");
        engine.input(b"\x08\x1b[K\r> \x1b[C");

        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, '>');
        assert_eq!(buf.grid.get(0, 1).c, ' ');
        assert_eq!(buf.grid.get(0, 2).c, 'a');
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(engine.state().cursor.col, 3);
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
    fn test_sgr_colour_keeps_bold() {
        // Regression: files_rs (like every TUI) writes `ESC[1m` and then a
        // colour sequence. The colour must not clear the bold.
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"\x1b[1m\x1b[38;5;2;49m");
        assert!(engine.state().attributes.bold);
        assert_eq!(engine.state().fg_color, Color::Palette(2));

        engine.input(b"\x1b[0m");
        assert!(!engine.state().attributes.bold);
    }

    #[test]
    fn test_sgr_attributes_survive_colour_and_turn_off_individually() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"\x1b[3;4m");
        assert!(engine.state().attributes.italic);
        assert!(engine.state().attributes.underline);

        engine.input(b"\x1b[31m"); // colour only
        assert!(engine.state().attributes.italic);
        assert!(engine.state().attributes.underline);

        engine.input(b"\x1b[23m"); // italic off
        assert!(!engine.state().attributes.italic);
        assert!(engine.state().attributes.underline);
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
    fn test_prompt_does_not_overlap_output_after_scroll() {
        // Regression: when output fills the screen, the lines must scroll up so
        // the prompt lands on a fresh line instead of overwriting the last one.
        let mut engine = TerminalEngine::new(3, 10);
        engine.input(b"a\r\nb\r\nc\r\n");
        engine.input(b"$ ");

        let buf = engine.buffer().active();
        let row = |r: usize| -> String {
            (0..buf.grid.cols)
                .map(|c| buf.grid.get(r, c).c)
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        assert_eq!(row(1), "c");
        assert_eq!(row(2), "$");
    }

    #[test]
    fn test_wrap_around() {
        let mut engine = TerminalEngine::new(1, 5);
        engine.input(b"ABCDE");
        // Deferred wrap: the cursor stays in the last column until the next
        // character is actually written.
        assert_eq!(engine.state().cursor.col, 4);
        assert_eq!(engine.state().cursor.row, 0);

        engine.input(b"F");
        // 'F' wraps to a new line, scrolling the single-row screen.
        let buf = engine.buffer().active();
        assert_eq!(buf.grid.rows, 1);
        assert_eq!(buf.grid.get(0, 0).c, 'F');
    }

    #[test]
    fn test_full_width_line_occupies_one_row() {
        // Regression: `top` pads every line to the full width. With immediate
        // wrapping that consumed two rows per line, scrolling the header away.
        let mut engine = TerminalEngine::new(4, 5);
        engine.input(b"AAAAA\r\nBBBBB\r\n");

        let buf = engine.buffer().active();
        let row =
            |r: usize| -> String { (0..buf.grid.cols).map(|c| buf.grid.get(r, c).c).collect() };
        assert_eq!(row(0), "AAAAA");
        assert_eq!(row(1), "BBBBB");
        assert_eq!(row(2), "     ");
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

    #[test]
    fn test_vpa_keeps_column() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"\x1b[6;11H"); // row 5, column 10
        engine.input(b"\x1b[20d"); // VPA: row 19, column unchanged

        assert_eq!(engine.state().cursor.row, 19);
        assert_eq!(engine.state().cursor.col, 10);
    }

    #[test]
    fn test_vpa_clamps_to_screen() {
        let mut engine = TerminalEngine::new(4, 10);
        engine.input(b"\x1b[99d");
        assert_eq!(engine.state().cursor.row, 3);
    }

    #[test]
    fn test_ech_erases_without_moving_cursor() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"abcdef");
        engine.input(b"\x1b[3D"); // back to column 3
        engine.input(b"\x1b[2X"); // erase two characters

        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 2).c, 'c');
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(buf.grid.get(0, 4).c, ' ');
        assert_eq!(buf.grid.get(0, 5).c, 'f');
        assert_eq!(engine.state().cursor.col, 3, "ECH must not move the cursor");
    }

    #[test]
    fn test_htop_style_row_stepping() {
        // Regression: htop writes a row, then `CR` + `ESC [ <row> d` (VPA) to
        // step to the next one. Ignoring the VPA stacked the header and the
        // process rows on the same line.
        let mut engine = TerminalEngine::new(6, 10);
        engine.input(b"\x1b[3dheader\r\x1b[4drow");

        let buf = engine.buffer().active();
        let row =
            |r: usize| -> String { (0..buf.grid.cols).map(|c| buf.grid.get(r, c).c).collect() };
        assert_eq!(row(2).trim_end(), "header");
        assert_eq!(row(3).trim_end(), "row");
    }

    #[test]
    fn test_resize_reflows_grid() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.input(b"hello");

        engine.resize(5, 10);
        assert_eq!(engine.dimensions(), (5, 10));

        let buf = engine.buffer().active();
        assert_eq!(buf.grid.get(0, 0).c, 'h');
        assert_eq!(buf.grid.get(0, 4).c, 'o');

        // Writing continues at the preserved cursor position.
        engine.input(b"!");
        assert_eq!(engine.buffer().active().grid.get(0, 5).c, '!');
    }
}
