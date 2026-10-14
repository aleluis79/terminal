//! Scrollback buffer management.
//!
//! The buffer consists of:
//! - An active grid (visible screen)
//! - A scrollback history (VecDeque of grids)
//! - Support for alternate screen mode

use std::collections::VecDeque;

use crate::cell::Cell;
use crate::grid::Grid;

/// Maximum number of lines in scrollback.
const DEFAULT_SCROLLBACK_LIMIT: usize = 10_000;

/// A single screen buffer (grid + cursor position).
#[derive(Debug, Clone)]
pub struct ScreenBuffer {
    /// The grid content.
    pub grid: Grid,
    /// Cursor position within this buffer.
    pub cursor_row: usize,
    /// Cursor position within this buffer.
    pub cursor_col: usize,
}

impl ScreenBuffer {
    /// Create a new screen buffer with the given dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            grid: Grid::new(rows, cols),
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    /// Get the current cursor position.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// Set the cursor position.
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row;
        self.cursor_col = col;
    }

    /// Resize the buffer, preserving the content that still fits.
    ///
    /// Content is anchored at the top-left; the cursor is clamped to the new
    /// bounds.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        if rows == self.grid.rows && cols == self.grid.cols {
            return;
        }
        let old_rows = self.grid.rows;
        let old_cols = self.grid.cols;

        let mut cells = vec![Cell::default(); rows * cols];
        let copy_rows = rows.min(old_rows);
        let copy_cols = cols.min(old_cols);
        for row in 0..copy_rows {
            for col in 0..copy_cols {
                cells[row * cols + col] = self.grid.cells[row * old_cols + col];
            }
        }

        self.grid.rows = rows;
        self.grid.cols = cols;
        self.grid.cells = cells;
        // A truncated row can split a wide glyph; drop any orphaned half.
        self.grid.sanitize_wide();
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    /// Scroll the active grid up by `n` lines.
    ///
    /// Lines are shifted towards the top and the `n` rows revealed at the
    /// bottom are blanked. This is the primitive behind line feed at the
    /// bottom of the screen.
    pub fn scroll_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let rows = self.grid.rows;
        let cols = self.grid.cols;
        let shift = n.min(rows);

        // Shift the surviving lines up by `shift` rows.
        if shift < rows {
            self.grid.cells.copy_within(shift * cols..rows * cols, 0);
        }
        // Blank the rows revealed at the bottom.
        self.grid.cells[(rows - shift) * cols..].fill(Cell::default());

        self.cursor_row = self.cursor_row.saturating_sub(shift);
    }

    /// Scroll down (bring back from scrollback).
    pub fn scroll_down(&mut self) {
        // In a full implementation, this would bring back lines from scrollback
        // For now, it's a no-op (scrollback management is in GridBuffer)
    }

    /// Insert n blank lines at cursor position.
    pub fn insert_lines(&mut self, n: usize) {
        let rows = self.grid.rows;
        let cols = self.grid.cols;
        let start = self.cursor_row;
        let end = (start + n).min(rows);

        if end > rows {
            return;
        }

        // Shift lines down
        for row in (start..end).rev() {
            for col in 0..cols {
                self.grid.cells[(row + 1) * cols + col] = self.grid.cells[row * cols + col];
            }
        }

        // Fill inserted lines with empty cells
        for row in start..end {
            for col in 0..cols {
                self.grid.cells[row * cols + col] = Cell::default();
            }
        }
    }

    /// Delete n lines at cursor position.
    pub fn delete_lines(&mut self, n: usize) {
        let rows = self.grid.rows;
        let cols = self.grid.cols;
        let start = self.cursor_row;

        if start >= rows {
            return;
        }

        let end = (start + n).min(rows);
        let shift = end - start;

        // Shift lines up
        for row in start..(rows - shift) {
            for col in 0..cols {
                self.grid.cells[row * cols + col] = self.grid.cells[(row + shift) * cols + col];
            }
        }

        // Fill bottom lines with empty cells
        for row in (rows - shift)..rows {
            for col in 0..cols {
                self.grid.cells[row * cols + col] = Cell::default();
            }
        }
    }

    /// Advance cursor and wrap to next line if needed.
    pub fn advance_and_wrap(&mut self) {
        self.cursor_col += 1;
        if self.cursor_col >= self.grid.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.grid.rows {
                self.scroll_up(1);
            }
        }
    }

    /// Clear the active region from cursor to end of screen.
    pub fn clear_from_cursor(&mut self) {
        // Clear from cursor to end of current line, keeping wide pairs whole.
        for col in self.cursor_col..self.grid.cols {
            self.grid.clear_cell(self.cursor_row, col);
        }
        // Clear all lines below
        for row in (self.cursor_row + 1)..self.grid.rows {
            for col in 0..self.grid.cols {
                self.grid.cells[row * self.grid.cols + col] = Cell::default();
            }
        }
    }

    /// Clear the entire screen.
    pub fn clear_screen(&mut self) {
        self.grid.clear();
    }

    /// Clear the current line.
    pub fn clear_line(&mut self) {
        let cols = self.grid.cols;
        for col in self.cursor_col..cols {
            self.grid.clear_cell(self.cursor_row, col);
        }
    }

    /// Erase part of a line (EL).
    ///
    /// `mode`: `0` = cursor→end, `1` = start→cursor (inclusive), `2` = whole line.
    ///
    /// Erasing either half of a wide character clears its other half too (RF-45).
    pub fn erase_in_line(&mut self, row: usize, col: usize, mode: u8) {
        if row >= self.grid.rows {
            return;
        }
        let cols = self.grid.cols;
        let (start, end) = match mode {
            1 => (0, (col + 1).min(cols)),
            2 => (0, cols),
            _ => (col.min(cols), cols),
        };
        for c in start..end {
            self.grid.clear_cell(row, c);
        }
    }

    /// Erase `n` characters starting at the cursor, without moving it (ECH).
    pub fn erase_chars(&mut self, row: usize, col: usize, n: usize) {
        if row >= self.grid.rows {
            return;
        }
        let cols = self.grid.cols;
        let end = col.saturating_add(n).min(cols);
        for c in col.min(cols)..end {
            self.grid.clear_cell(row, c);
        }
    }

    /// Erase part of the display (ED).
    ///
    /// `mode`: `0` = cursor→end, `1` = start→cursor (inclusive), `2` = whole screen.
    pub fn erase_in_display(&mut self, row: usize, col: usize, mode: u8) {
        match mode {
            1 => {
                for r in 0..row.min(self.grid.rows) {
                    self.erase_in_line(r, 0, 2);
                }
                self.erase_in_line(row, col, 1);
            }
            2 => self.grid.clear(),
            _ => {
                self.erase_in_line(row, col, 0);
                for r in (row + 1)..self.grid.rows {
                    self.erase_in_line(r, 0, 2);
                }
            }
        }
    }
}

/// The complete buffer manager: active screen + scrollback + alternate screen.
#[derive(Debug, Clone)]
pub struct GridBuffer {
    /// Main screen buffer.
    main: ScreenBuffer,
    /// Scrollback history.
    scrollback: VecDeque<ScrollbackEntry>,
    /// Alternate screen buffer (when `alternate_screen` mode is active).
    alternate: Option<ScreenBuffer>,
    /// Scrollback limit.
    scrollback_limit: usize,
}

#[derive(Debug, Clone)]
#[expect(dead_code)]
struct ScrollbackEntry {
    /// The grid content.
    grid: Grid,
    /// Cursor row position.
    cursor_row: usize,
    /// Cursor column position.
    cursor_col: usize,
}

impl GridBuffer {
    /// Create a new grid buffer with the given dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            main: ScreenBuffer::new(rows, cols),
            scrollback: VecDeque::new(),
            alternate: None,
            scrollback_limit: DEFAULT_SCROLLBACK_LIMIT,
        }
    }

    /// Get a reference to the active screen buffer.
    ///
    /// # Panics
    ///
    /// Panics if alternate screen is active but the alternate buffer doesn't exist.
    pub fn active(&self) -> &ScreenBuffer {
        if self.is_alternate_screen_active() {
            self.alternate
                .as_ref()
                .expect("alternate buffer should exist")
        } else {
            &self.main
        }
    }

    /// Get a mutable reference to the active screen buffer.
    ///
    /// # Panics
    ///
    /// Panics if alternate screen is active but the alternate buffer doesn't exist.
    pub fn active_mut(&mut self) -> &mut ScreenBuffer {
        if self.is_alternate_screen_active() {
            self.alternate
                .as_mut()
                .expect("alternate buffer should exist")
        } else {
            &mut self.main
        }
    }

    /// Check if alternate screen is active.
    pub fn is_alternate_screen_active(&self) -> bool {
        self.alternate.is_some()
    }

    /// Enter alternate screen mode.
    pub fn enter_alternate_screen(&mut self) {
        if self.alternate.is_none() {
            // Save main screen to scrollback before switching
            self.save_to_scrollback();
            self.alternate = Some(ScreenBuffer::new(self.main.grid.rows, self.main.grid.cols));
        }
    }

    /// Exit alternate screen mode.
    pub fn exit_alternate_screen(&mut self) {
        self.alternate = None;
    }

    /// Resize both the main and alternate screen buffers.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        if rows == 0 || cols == 0 {
            return;
        }
        self.main.resize(rows, cols);
        if let Some(alt) = self.alternate.as_mut() {
            alt.resize(rows, cols);
        }
    }

    /// Save current main screen to scrollback.
    fn save_to_scrollback(&mut self) {
        let entry = ScrollbackEntry {
            grid: self.main.grid.clone(),
            cursor_row: self.main.cursor_row,
            cursor_col: self.main.cursor_col,
        };
        self.scrollback.push_back(entry);
        // Limit scrollback size
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
        }
    }

    /// Get scrollback line count.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Get a scrollback line by index (0 = oldest).
    pub fn scrollback_line(&self, index: usize) -> Option<Grid> {
        self.scrollback.get(index).map(|e| e.grid.clone())
    }

    /// Clear scrollback.
    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
    }

    /// Handle a line feed on the main buffer.
    pub fn main_line_feed(&mut self) {
        if self.main.cursor_row + 1 >= self.main.grid.rows {
            self.main.scroll_up(1);
            // After scroll, cursor stays at the last row
            self.main.cursor_row = self.main.grid.rows - 1;
        } else {
            self.main.cursor_row += 1;
        }
    }

    /// Handle a line feed on the alternate buffer.
    pub fn alternate_line_feed(&mut self) {
        if let Some(ref mut alt) = self.alternate {
            if alt.cursor_row + 1 >= alt.grid.rows {
                alt.scroll_up(1);
                // After scroll, cursor stays at the last row
                alt.cursor_row = alt.grid.rows - 1;
            } else {
                alt.cursor_row += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Cell;

    #[test]
    fn test_screen_buffer_creation() {
        let buf = ScreenBuffer::new(24, 80);
        assert_eq!(buf.grid.rows, 24);
        assert_eq!(buf.grid.cols, 80);
        assert_eq!(buf.cursor(), (0, 0));
    }

    #[test]
    fn test_set_cursor() {
        let mut buf = ScreenBuffer::new(24, 80);
        buf.set_cursor(10, 20);
        assert_eq!(buf.cursor(), (10, 20));
    }

    #[test]
    fn test_resize_preserves_content() {
        let mut buf = ScreenBuffer::new(3, 4);
        buf.grid.cells[0] = Cell {
            c: 'A',
            ..Cell::default()
        };
        buf.grid.cells[5] = Cell {
            c: 'B',
            ..Cell::default()
        };
        buf.set_cursor(2, 3);

        buf.resize(2, 2);

        assert_eq!((buf.grid.rows, buf.grid.cols), (2, 2));
        assert_eq!(buf.grid.get(0, 0).c, 'A');
        assert_eq!(buf.grid.get(1, 1).c, 'B');
        // The cursor is clamped into the new bounds.
        assert_eq!(buf.cursor(), (1, 1));
    }

    #[test]
    fn test_resize_grow_pads_with_blanks() {
        let mut buf = ScreenBuffer::new(2, 2);
        buf.grid.cells[0] = Cell {
            c: 'X',
            ..Cell::default()
        };

        buf.resize(4, 5);

        assert_eq!((buf.grid.rows, buf.grid.cols), (4, 5));
        assert_eq!(buf.grid.get(0, 0).c, 'X');
        assert_eq!(buf.grid.get(3, 4).c, ' ');
    }

    #[test]
    fn test_scroll_up() {
        let mut buf = ScreenBuffer::new(3, 5);
        // Row 0 = 'A', row 1 = 'B', row 2 = 'C'.
        for (row, ch) in [(0usize, 'A'), (1, 'B'), (2, 'C')] {
            for col in 0..5 {
                buf.grid.cells[row * 5 + col] = Cell {
                    c: ch,
                    ..Cell::default()
                };
            }
        }

        buf.scroll_up(1);

        // Content shifts up one row and the bottom row is blanked.
        for col in 0..5 {
            assert_eq!(buf.grid.cells[col].c, 'B');
            assert_eq!(buf.grid.cells[5 + col].c, 'C');
            assert_eq!(buf.grid.cells[10 + col].c, ' ');
        }
    }

    #[test]
    fn test_scroll_up_multiple_lines() {
        let mut buf = ScreenBuffer::new(4, 3);
        for (row, ch) in [(0usize, 'A'), (1, 'B'), (2, 'C'), (3, 'D')] {
            for col in 0..3 {
                buf.grid.cells[row * 3 + col] = Cell {
                    c: ch,
                    ..Cell::default()
                };
            }
        }

        buf.scroll_up(2);

        // 'C' and 'D' survive, shifted to the top; the last two rows are blank.
        for col in 0..3 {
            assert_eq!(buf.grid.cells[col].c, 'C');
            assert_eq!(buf.grid.cells[3 + col].c, 'D');
            assert_eq!(buf.grid.cells[6 + col].c, ' ');
            assert_eq!(buf.grid.cells[9 + col].c, ' ');
        }
    }

    #[test]
    fn test_advance_and_wrap() {
        let mut buf = ScreenBuffer::new(1, 5);
        buf.cursor_col = 3;
        buf.advance_and_wrap();
        assert_eq!(buf.cursor_col, 4);
        buf.advance_and_wrap();
        assert_eq!(buf.cursor_col, 0);
    }

    #[test]
    fn test_clear_from_cursor() {
        let mut buf = ScreenBuffer::new(3, 5);
        // Fill with 'X'
        for row in 0..3 {
            for col in 0..5 {
                buf.grid.cells[row * 5 + col] = Cell {
                    c: 'X',
                    ..Cell::default()
                };
            }
        }
        buf.cursor_row = 1;
        buf.cursor_col = 2;
        buf.clear_from_cursor();
        // Row 0 should still have 'X'
        for col in 0..5 {
            assert_eq!(buf.grid.cells[col].c, 'X');
        }
        // Row 1, cols 0-1 should have 'X'
        assert_eq!(buf.grid.cells[5].c, 'X');
        assert_eq!(buf.grid.cells[6].c, 'X');
        // Row 1, cols 2-4 should be empty
        for col in 2..5 {
            assert_eq!(buf.grid.cells[5 + col].c, ' ');
        }
        // Rows 2 should be empty
        for col in 0..5 {
            assert_eq!(buf.grid.cells[10 + col].c, ' ');
        }
    }

    #[test]
    fn test_clear_screen() {
        let mut buf = ScreenBuffer::new(3, 5);
        for row in 0..3 {
            for col in 0..5 {
                buf.grid.cells[row * 5 + col] = Cell {
                    c: 'X',
                    ..Cell::default()
                };
            }
        }
        buf.clear_screen();
        for row in 0..3 {
            for col in 0..5 {
                assert_eq!(buf.grid.cells[row * 5 + col].c, ' ');
            }
        }
    }

    #[test]
    fn test_grid_buffer_alternate_screen() {
        let mut buf = GridBuffer::new(24, 80);
        assert!(!buf.is_alternate_screen_active());
        buf.enter_alternate_screen();
        assert!(buf.is_alternate_screen_active());
        buf.exit_alternate_screen();
        assert!(!buf.is_alternate_screen_active());
    }

    #[test]
    fn test_grid_buffer_active_ref() {
        let mut buf = GridBuffer::new(24, 80);
        let active = buf.active_mut();
        active.set_cursor(5, 10);
        assert_eq!(buf.active().cursor(), (5, 10));
    }

    #[test]
    fn test_insert_lines() {
        let mut buf = ScreenBuffer::new(5, 5);
        // Fill with 'A'
        for row in 0..5 {
            for col in 0..5 {
                buf.grid.cells[row * 5 + col] = Cell {
                    c: 'A',
                    ..Cell::default()
                };
            }
        }
        buf.cursor_row = 1;
        buf.insert_lines(1);
        // Row 1 should now be empty
        for col in 0..5 {
            assert_eq!(buf.grid.cells[5 + col].c, ' ');
        }
        // Row 2 should have 'A' (was row 1)
        for col in 0..5 {
            assert_eq!(buf.grid.cells[10 + col].c, 'A');
        }
    }

    #[test]
    fn test_delete_lines() {
        let mut buf = ScreenBuffer::new(5, 5);
        // Fill with 'A'
        for row in 0..5 {
            for col in 0..5 {
                buf.grid.cells[row * 5 + col] = Cell {
                    c: 'A',
                    ..Cell::default()
                };
            }
        }
        buf.cursor_row = 1;
        buf.delete_lines(1);
        // Row 1 should have 'A' (was row 2)
        for col in 0..5 {
            assert_eq!(buf.grid.cells[5 + col].c, 'A');
        }
        // Last row should be empty
        for col in 0..5 {
            assert_eq!(buf.grid.cells[20 + col].c, ' ');
        }
    }

    #[test]
    fn test_main_line_feed() {
        let mut buf = GridBuffer::new(3, 5);
        buf.main.cursor_row = 2;
        buf.main_line_feed();
        assert_eq!(buf.main.cursor_row, 2); // at bottom, scrolls
    }

    #[test]
    fn test_scrollback_limit() {
        let mut buf = GridBuffer::new(3, 5);
        buf.scrollback_limit = 5;
        for _ in 0..10 {
            buf.save_to_scrollback();
        }
        assert!(buf.scrollback_len() <= 5);
    }

    #[test]
    fn test_erase_chars() {
        let mut buf = ScreenBuffer::new(2, 10);
        for (col, c) in "abcdefgh".chars().enumerate() {
            buf.grid.set(
                0,
                col,
                Cell {
                    c,
                    ..Cell::default()
                },
            );
        }

        buf.erase_chars(0, 2, 3);

        assert_eq!(buf.grid.get(0, 1).c, 'b');
        assert_eq!(buf.grid.get(0, 2).c, ' ');
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(buf.grid.get(0, 4).c, ' ');
        assert_eq!(buf.grid.get(0, 5).c, 'f');
    }

    #[test]
    fn test_erase_chars_clamps_to_line_end() {
        let mut buf = ScreenBuffer::new(2, 4);
        buf.grid.set(
            0,
            0,
            Cell {
                c: 'x',
                ..Cell::default()
            },
        );

        // Asking for more characters than fit must stop at the last column.
        buf.erase_chars(0, 2, 100);
        assert_eq!(buf.grid.get(0, 0).c, 'x');
        assert_eq!(buf.grid.get(0, 3).c, ' ');

        // Rows outside the screen are ignored instead of panicking.
        buf.erase_chars(9, 0, 4);
    }

    #[test]
    fn test_erase_line_clears_wide_pair() {
        // Erasing only the tail of a wide character must remove its head too.
        let mut buf = ScreenBuffer::new(2, 10);
        buf.grid.set_wide(
            0,
            2,
            Cell {
                c: '日',
                ..Cell::default()
            },
        );

        buf.erase_chars(0, 3, 1); // erase the continuation half only

        assert_eq!(buf.grid.get(0, 2).c, ' ');
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(buf.grid.get(0, 2).width, 1);
    }

    #[test]
    fn test_clear_line_clears_wide_pair() {
        let mut buf = ScreenBuffer::new(2, 10);
        buf.grid.set_wide(
            0,
            4,
            Cell {
                c: '本',
                ..Cell::default()
            },
        );
        // Cursor on the head of the wide glyph; the tail belongs to the line.
        buf.set_cursor(0, 4);
        buf.clear_line();
        assert_eq!(buf.grid.get(0, 4).c, ' ');
        assert_eq!(buf.grid.get(0, 5).c, ' ');
    }

    #[test]
    fn test_resize_drops_split_wide_glyph() {
        // Shrinking the line so only the left half of a wide glyph fits must
        // blank it, never leave a half behind (CL-4).
        let mut buf = ScreenBuffer::new(2, 5);
        buf.grid.set_wide(
            0,
            3,
            Cell {
                c: '日',
                ..Cell::default()
            },
        );

        buf.resize(2, 4); // column 4 (the continuation) is truncated

        assert!(!buf.grid.is_continuation(0, 3));
        assert_eq!(buf.grid.get(0, 3).c, ' ');
        assert_eq!(buf.grid.get(0, 3).width, 1);
    }

    #[test]
    fn test_resize_keeps_complete_wide_glyph() {
        let mut buf = ScreenBuffer::new(2, 5);
        buf.grid.set_wide(
            0,
            1,
            Cell {
                c: '本',
                ..Cell::default()
            },
        );

        buf.resize(2, 6);

        assert_eq!(buf.grid.get(0, 1).c, '本');
        assert!(buf.grid.is_continuation(0, 2));
    }
}
