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

    /// Scroll up by n lines (push active to scrollback).
    pub fn scroll_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        // Move the visible portion to scrollback
        let visible: Vec<Cell> = self.grid.cells.clone();
        // Shift grid content down by n lines
        let rows = self.grid.rows;
        let cols = self.grid.cols;
        let shift = n.min(rows);

        let mut new_cells = vec![Cell::default(); rows * cols];
        // Copy lines that remain visible (after scrolling)
        for row in shift..rows {
            for col in 0..cols {
                new_cells[row * cols + col] = visible[row * cols + col];
            }
        }
        self.grid.cells = new_cells;
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
        let cols = self.grid.cols;
        // Clear from cursor to end of current line
        for col in self.cursor_col..cols {
            self.grid.cells[self.cursor_row * cols + col] = Cell::default();
        }
        // Clear all lines below
        for row in (self.cursor_row + 1)..self.grid.rows {
            for col in 0..cols {
                self.grid.cells[row * cols + col] = Cell::default();
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
            self.grid.cells[self.cursor_row * cols + col] = Cell::default();
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
    fn test_scroll_up() {
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
        buf.scroll_up(1);
        // First row should now be empty
        for col in 0..5 {
            assert_eq!(buf.grid.cells[col].c, ' ');
        }
        // Second row should have 'A'
        for col in 0..5 {
            assert_eq!(buf.grid.cells[5 + col].c, 'A');
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
}
