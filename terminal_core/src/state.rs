//! Terminal state: cursor position, modes, and text attributes.

use crate::cell::Color;

/// The shape of the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// Block cursor (▉).
    #[default]
    Block,
    /// Underline cursor (─).
    Underline,
    /// Beam cursor (│).
    Beam,
}

/// Cursor position in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    /// Row position (0-indexed).
    pub row: usize,
    /// Column position (0-indexed).
    pub col: usize,
    /// Cursor shape.
    pub shape: CursorShape,
}

impl Cursor {
    /// Create a new cursor at (row, col) with default shape.
    pub fn new(row: usize, col: usize) -> Self {
        Self {
            row,
            col,
            shape: CursorShape::default(),
        }
    }

    /// Move cursor to absolute position.
    pub fn move_to(&mut self, row: usize, col: usize) {
        self.row = row;
        self.col = col;
    }

    /// Move cursor down by n rows.
    pub fn move_down(&mut self, n: usize) {
        self.row = self.row.saturating_add(n);
    }

    /// Move cursor right by n columns.
    pub fn move_right(&mut self, n: usize) {
        self.col = self.col.saturating_add(n);
    }

    /// Move cursor up by n rows.
    pub fn move_up(&mut self, n: usize) {
        self.row = self.row.saturating_sub(n);
    }

    /// Move cursor left by n columns.
    pub fn move_left(&mut self, n: usize) {
        self.col = self.col.saturating_sub(n);
    }
}

/// Terminal operating modes.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalModes {
    /// Line wrap mode (DECARM).
    pub wraparound: bool,
    /// Insert mode (DECOM).
    pub insert: bool,
    /// Origin mode (cursor relative to margins).
    pub origin: bool,
    /// Alternate screen buffer.
    pub alternate_screen: bool,
    /// Bracketed paste mode.
    pub bracketed_paste: bool,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            wraparound: true, // DECARM is on by default
            insert: false,
            origin: false,
            alternate_screen: false,
            bracketed_paste: false,
        }
    }
}

impl TerminalModes {
    /// Enable a mode by DECSET number.
    pub fn decset(&mut self, mode: u16, enable: bool) {
        match mode {
            4 => self.insert = enable,
            20 => self.bracketed_paste = enable,
            _ => {}
        }
        // Common modes
        match (mode, enable) {
            (1, true) => self.origin = true,
            (1, false) => self.origin = false,
            (7, true) => self.wraparound = true,
            (7, false) => self.wraparound = false,
            (47 | 1047 | 1049, true) => self.alternate_screen = true,
            (47 | 1047 | 1049, false) => self.alternate_screen = false,
            _ => {}
        }
    }

    /// Check if a mode is set.
    pub fn is_set(&self, mode: u16) -> bool {
        match mode {
            4 => self.insert,
            7 => self.wraparound,
            20 => self.bracketed_paste,
            47 | 1047 | 1049 => self.alternate_screen,
            _ => false,
        }
    }
}

/// Text attributes (SGR parameters).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextAttributes {
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
    /// Inverted text.
    pub inverse: bool,
    /// Hidden text.
    pub hidden: bool,
    /// Strikethrough text.
    pub strikethrough: bool,
}

impl TextAttributes {
    /// Apply SGR parameter.
    pub fn apply_sgr(&mut self, param: u16) {
        match param {
            0 => *self = TextAttributes::default(),
            1 => self.bold = true,
            2 => self.hidden = true,
            3 => self.italic = true,
            4 => self.underline = true,
            7 => self.inverse = true,
            9 => self.strikethrough = true,
            21 | 24 => self.underline = false,
            22 => {
                self.bold = false;
                self.hidden = false;
            }
            23 => self.italic = false,
            27 => self.inverse = false,
            29 => self.strikethrough = false,
            _ => {}
        }
    }
}

/// Complete terminal state.
#[derive(Debug, Clone)]
pub struct TerminalState {
    /// Cursor position and shape.
    pub cursor: Cursor,
    /// Current terminal modes.
    pub modes: TerminalModes,
    /// Current text attributes.
    pub attributes: TextAttributes,
    /// Current foreground color.
    pub fg_color: Color,
    /// Current background color.
    pub bg_color: Color,
    /// Default foreground color.
    pub default_fg: Color,
    /// Default background color.
    pub default_bg: Color,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            cursor: Cursor::new(0, 0),
            modes: TerminalModes::default(),
            attributes: TextAttributes::default(),
            fg_color: Color::Named(7),
            bg_color: Color::Named(0),
            default_fg: Color::Named(7),
            default_bg: Color::Named(0),
        }
    }
}

impl TerminalState {
    /// Reset state to defaults.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Reset text attributes and colors to defaults (SGR 0).
    ///
    /// Unlike [`reset`](Self::reset), this preserves the cursor position and
    /// terminal modes, which is what `ESC [ 0 m` actually does.
    pub fn reset_attributes(&mut self) {
        self.attributes = TextAttributes::default();
        self.fg_color = self.default_fg;
        self.bg_color = self.default_bg;
    }

    /// Move cursor to cell and write character.
    pub fn write_char(&mut self, _c: char) {
        // Cursor position is managed by the grid/engine
        // This records the character to be written at cursor position
        self.cursor.col = self.cursor.col.saturating_add(1);
    }

    /// Handle line feed.
    pub fn line_feed(&mut self) {
        self.cursor.row = self.cursor.row.saturating_add(1);
    }

    /// Handle carriage return.
    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    /// Handle backspace.
    pub fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col = self.cursor.col.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_creation() {
        let cursor = Cursor::new(5, 10);
        assert_eq!(cursor.row, 5);
        assert_eq!(cursor.col, 10);
        assert_eq!(cursor.shape, CursorShape::Block);
    }

    #[test]
    fn test_cursor_move_to() {
        let mut cursor = Cursor::new(0, 0);
        cursor.move_to(10, 20);
        assert_eq!(cursor.row, 10);
        assert_eq!(cursor.col, 20);
    }

    #[test]
    fn test_cursor_move_down() {
        let mut cursor = Cursor::new(5, 5);
        cursor.move_down(3);
        assert_eq!(cursor.row, 8);
    }

    #[test]
    fn test_cursor_move_up() {
        let mut cursor = Cursor::new(5, 5);
        cursor.move_up(10);
        assert_eq!(cursor.row, 0); // saturates at 0
    }

    #[test]
    fn test_cursor_move_right() {
        let mut cursor = Cursor::new(5, 5);
        cursor.move_right(3);
        assert_eq!(cursor.col, 8);
    }

    #[test]
    fn test_cursor_move_left() {
        let mut cursor = Cursor::new(5, 5);
        cursor.move_left(10);
        assert_eq!(cursor.col, 0); // saturates at 0
    }

    #[test]
    fn test_modes_decset() {
        let mut modes = TerminalModes::default();
        assert!(modes.wraparound); // DECARM is on by default
        modes.decset(7, false);
        assert!(!modes.wraparound);
        modes.decset(7, true);
        assert!(modes.wraparound);
    }

    #[test]
    fn test_modes_alternate_screen() {
        let mut modes = TerminalModes::default();
        assert!(!modes.alternate_screen);
        modes.decset(1049, true);
        assert!(modes.alternate_screen);
        modes.decset(1049, false);
        assert!(!modes.alternate_screen);
    }

    #[test]
    fn test_modes_bracketed_paste() {
        let mut modes = TerminalModes::default();
        assert!(!modes.bracketed_paste);
        modes.decset(20, true);
        assert!(modes.bracketed_paste);
    }

    #[test]
    fn test_modes_is_set() {
        let mut modes = TerminalModes::default();
        modes.decset(7, true);
        assert!(modes.is_set(7));
        assert!(!modes.is_set(20));
    }

    #[test]
    fn test_attributes_sgr_reset() {
        let mut attrs = TextAttributes {
            bold: true,
            italic: true,
            underline: true,
            ..TextAttributes::default()
        };
        attrs.apply_sgr(0); // reset
        assert!(!attrs.bold);
        assert!(!attrs.italic);
        assert!(!attrs.underline);
    }

    #[test]
    fn test_attributes_sgr_bold() {
        let mut attrs = TextAttributes::default();
        attrs.apply_sgr(1);
        assert!(attrs.bold);
    }

    #[test]
    fn test_attributes_sgr_italic() {
        let mut attrs = TextAttributes::default();
        attrs.apply_sgr(3);
        assert!(attrs.italic);
    }

    #[test]
    fn test_state_default() {
        let state = TerminalState::default();
        assert_eq!(state.cursor.row, 0);
        assert_eq!(state.cursor.col, 0);
        assert!(state.modes.wraparound); // default is true for wraparound
    }

    #[test]
    fn test_state_reset() {
        let mut state = TerminalState::default();
        state.cursor.move_to(10, 20);
        state.attributes.bold = true;
        state.reset();
        assert_eq!(state.cursor.row, 0);
        assert!(!state.attributes.bold);
    }

    #[test]
    fn test_write_char_increments_col() {
        let mut state = TerminalState::default();
        state.cursor.col = 5;
        state.write_char('A');
        assert_eq!(state.cursor.col, 6);
    }

    #[test]
    fn test_line_feed_increments_row() {
        let mut state = TerminalState::default();
        state.cursor.row = 10;
        state.line_feed();
        assert_eq!(state.cursor.row, 11);
    }

    #[test]
    fn test_carriage_return_resets_col() {
        let mut state = TerminalState::default();
        state.cursor.col = 80;
        state.carriage_return();
        assert_eq!(state.cursor.col, 0);
    }

    #[test]
    fn test_backspace_decrements_col() {
        let mut state = TerminalState::default();
        state.cursor.col = 5;
        state.backspace();
        assert_eq!(state.cursor.col, 4);
    }

    #[test]
    fn test_backspace_at_zero_stays_zero() {
        let mut state = TerminalState::default();
        state.cursor.col = 0;
        state.backspace();
        assert_eq!(state.cursor.col, 0);
    }
}
