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
    /// First row of the scrolling region (DECSTBM), 0-indexed (RF-57).
    pub scroll_top: usize,
    /// Last row of the scrolling region (DECSTBM), inclusive (RF-57).
    pub scroll_bottom: usize,
}

impl ScreenBuffer {
    /// Create a new screen buffer with the given dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            grid: Grid::new(rows, cols),
            cursor_row: 0,
            cursor_col: 0,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
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

    /// Set the scrolling region (DECSTBM, RF-57).
    ///
    /// `top`/`bottom` are 0-indexed and inclusive. An invalid region
    /// (`top >= bottom`) is ignored, keeping the previous margins (CL-22).
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let last = self.grid.rows.saturating_sub(1);
        let top = top.min(last);
        let bottom = bottom.min(last);
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        }
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

        // Keep the scrolling region consistent with the new height: extend it
        // when it covered the whole screen, otherwise clamp it (RF-57).
        let last = rows.saturating_sub(1);
        let was_full = self.scroll_top == 0 && self.scroll_bottom == old_rows.saturating_sub(1);
        self.scroll_bottom = if was_full {
            last
        } else {
            self.scroll_bottom.min(last)
        };
        self.scroll_top = self.scroll_top.min(last);
        if self.scroll_top >= self.scroll_bottom {
            self.scroll_top = 0;
            self.scroll_bottom = last;
        }
    }

    /// The scrolling region as `(top, bottom)`, inclusive and clamped to the
    /// current grid (RF-57).
    fn scroll_region(&self) -> (usize, usize) {
        let last = self.grid.rows.saturating_sub(1);
        (self.scroll_top.min(last), self.scroll_bottom.min(last))
    }

    /// Scroll the active grid up by `n` lines, within the scrolling region.
    ///
    /// Lines are shifted towards the top and the `n` rows revealed at the
    /// bottom of the region are blanked. This is the primitive behind line feed
    /// at the bottom margin (RF-57).
    pub fn scroll_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let cols = self.grid.cols;
        let (top, bottom) = self.scroll_region();
        if top > bottom {
            return;
        }
        let height = bottom - top + 1;
        let shift = n.min(height);

        // Shift the surviving lines up by `shift` rows.
        if shift < height {
            self.grid
                .cells
                .copy_within((top + shift) * cols..(bottom + 1) * cols, top * cols);
        }
        // Blank the rows revealed at the bottom of the region.
        self.grid.cells[(bottom + 1 - shift) * cols..(bottom + 1) * cols].fill(Cell::default());

        self.cursor_row = self.cursor_row.saturating_sub(shift);
    }

    /// Scroll the active grid down by `n` lines, within the scrolling region.
    ///
    /// Lines are shifted towards the bottom and the `n` rows revealed at the
    /// top of the region are blanked. This is the primitive behind the reverse
    /// index (`ESC M`) at the top margin (RF-55, RF-57); unlike scrolling up it
    /// does not touch the scrollback and leaves the cursor where it is.
    pub fn scroll_down(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let cols = self.grid.cols;
        let (top, bottom) = self.scroll_region();
        if top > bottom {
            return;
        }
        let height = bottom - top + 1;
        let shift = n.min(height);

        // Shift the surviving lines down by `shift` rows.
        if shift < height {
            self.grid.cells.copy_within(
                top * cols..(bottom + 1 - shift) * cols,
                (top + shift) * cols,
            );
        }
        // Blank the rows revealed at the top of the region.
        self.grid.cells[top * cols..(top + shift) * cols].fill(Cell::default());
    }

    /// Insert `n` blank lines at the cursor, within the scrolling region
    /// (RF-57).
    ///
    /// Lines shift towards the region's bottom and the ones pushed past the
    /// bottom margin are discarded. If the cursor is outside the region the
    /// sequence is ignored, as in xterm.
    pub fn insert_lines(&mut self, n: usize) {
        let cols = self.grid.cols;
        let (top, bottom) = self.scroll_region();
        let start = self.cursor_row;
        if start < top || start > bottom {
            return;
        }
        let height = bottom - start + 1;
        let shift = n.min(height);

        // Shift lines down.
        if shift < height {
            self.grid.cells.copy_within(
                start * cols..(bottom + 1 - shift) * cols,
                (start + shift) * cols,
            );
        }
        // Fill the inserted lines with empty cells.
        self.grid.cells[start * cols..(start + shift) * cols].fill(Cell::default());
    }

    /// Delete `n` lines at the cursor, within the scrolling region (RF-57).
    ///
    /// Lines below the deleted ones shift up and the region's bottom rows are
    /// blanked. If the cursor is outside the region the sequence is ignored.
    pub fn delete_lines(&mut self, n: usize) {
        let cols = self.grid.cols;
        let (top, bottom) = self.scroll_region();
        let start = self.cursor_row;
        if start < top || start > bottom {
            return;
        }
        let height = bottom - start + 1;
        let shift = n.min(height);

        // Shift lines up.
        if shift < height {
            self.grid
                .cells
                .copy_within((start + shift) * cols..(bottom + 1) * cols, start * cols);
        }
        // Blank the region's bottom lines.
        self.grid.cells[(bottom + 1 - shift) * cols..(bottom + 1) * cols].fill(Cell::default());
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
    /// Scrollback history: whole lines (rows) that scrolled off the top, oldest
    /// first.
    scrollback: VecDeque<Vec<Cell>>,
    /// Alternate screen buffer (when `alternate_screen` mode is active).
    alternate: Option<ScreenBuffer>,
    /// Scrollback limit.
    scrollback_limit: usize,
    /// How many lines the view is scrolled back into the history.
    scroll_offset: usize,
}

impl GridBuffer {
    /// Create a new grid buffer with the given dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            main: ScreenBuffer::new(rows, cols),
            scrollback: VecDeque::new(),
            alternate: None,
            scrollback_limit: DEFAULT_SCROLLBACK_LIMIT,
            scroll_offset: 0,
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

    /// Append the top line of the main screen to the scrollback.
    ///
    /// Called when the screen scrolls up: the row that leaves the top becomes
    /// history. The oldest lines are dropped once the limit is reached.
    fn push_scrollback_line(&mut self) {
        let cols = self.main.grid.cols;
        let line: Vec<Cell> = (0..cols).map(|col| self.main.grid.get(0, col)).collect();
        self.scrollback.push_back(line);
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
        }
    }

    /// Get scrollback line count.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Get a scrollback line by index (0 = oldest).
    pub fn scrollback_line(&self, index: usize) -> Option<&[Cell]> {
        self.scrollback.get(index).map(Vec::as_slice)
    }

    /// Clear scrollback.
    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.scroll_offset = 0;
    }

    /// Handle a line feed on the main buffer.
    ///
    /// At the scrolling region's bottom margin the region scrolls up and the
    /// line that leaves the top of the *screen* becomes history — a partial
    /// region never feeds the scrollback (RF-57, CL-20). Below the region the
    /// cursor is clamped to its bottom margin.
    pub fn main_line_feed(&mut self) {
        let rows = self.main.grid.rows;
        let bottom = self.main.scroll_bottom.min(rows.saturating_sub(1));
        if self.main.cursor_row < bottom {
            self.main.cursor_row += 1;
        } else {
            if self.main.cursor_row == bottom {
                if self.main.scroll_top == 0 && bottom == rows.saturating_sub(1) {
                    self.push_scrollback_line();
                }
                self.main.scroll_up(1);
            }
            self.main.cursor_row = bottom;
        }
    }

    /// Handle a line feed on the alternate buffer.
    pub fn alternate_line_feed(&mut self) {
        if let Some(ref mut alt) = self.alternate {
            let rows = alt.grid.rows;
            let bottom = alt.scroll_bottom.min(rows.saturating_sub(1));
            if alt.cursor_row < bottom {
                alt.cursor_row += 1;
            } else {
                if alt.cursor_row == bottom {
                    alt.scroll_up(1);
                }
                alt.cursor_row = bottom;
            }
        }
    }

    /// Number of lines the view is scrolled back into the history (0 = bottom).
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Scroll the view back by `n` lines, towards older output (RF-53).
    ///
    /// The offset stops at the oldest history line (CL-15) and is ignored while
    /// the alternate screen is active, which has no history (CL-16).
    pub fn scroll_back(&mut self, n: usize) {
        if self.is_alternate_screen_active() {
            return;
        }
        self.scroll_offset = (self.scroll_offset + n).min(self.scrollback.len());
    }

    /// Scroll the view forward by `n` lines, towards the newest output.
    pub fn scroll_forward(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Jump to the bottom of the buffer (newest output).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Jump to the oldest line of the history (RF-53).
    pub fn scroll_to_top(&mut self) {
        if self.is_alternate_screen_active() {
            return;
        }
        self.scroll_offset = self.scrollback.len();
    }

    /// The rows to draw, as seen through the current scroll offset (RF-53).
    ///
    /// Returns `rows` rows: `scroll_offset` lines from the end of the history,
    /// followed by the top of the active grid.
    pub fn visible_rows(&self) -> Vec<Vec<Cell>> {
        let grid = &self.active().grid;
        let rows = grid.rows;
        let cols = grid.cols;
        let history = self.scrollback.len();
        let offset = self.scroll_offset.min(history);
        // Virtual line index where the window starts.
        let start = history - offset;

        let mut out = Vec::with_capacity(rows);
        for i in 0..rows {
            let line = start + i;
            if line < history {
                out.push(self.scrollback[line].clone());
            } else {
                let row = line - history;
                if row < rows {
                    out.push((0..cols).map(|col| grid.get(row, col)).collect());
                } else {
                    out.push(vec![Cell::default(); cols]);
                }
            }
        }
        out
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
            buf.push_scrollback_line();
        }
        assert!(buf.scrollback_len() <= 5);
    }

    #[test]
    fn test_main_line_feed_records_scrollback() {
        // A line that leaves the top of the screen becomes history.
        let mut buf = GridBuffer::new(2, 4);
        for (col, c) in "top!".chars().enumerate() {
            buf.main.grid.set(
                0,
                col,
                Cell {
                    c,
                    ..Cell::default()
                },
            );
        }
        buf.main.cursor_row = 1;
        buf.main_line_feed();

        assert_eq!(buf.scrollback_len(), 1);
        let line = buf.scrollback_line(0).expect("one history line");
        let text: String = line.iter().map(|cell| cell.c).collect();
        assert_eq!(text, "top!");
    }

    #[test]
    fn test_line_feed_without_scrolling_records_nothing() {
        let mut buf = GridBuffer::new(4, 4);
        buf.main.cursor_row = 0;
        buf.main_line_feed();
        assert_eq!(buf.scrollback_len(), 0);
    }

    #[test]
    fn test_visible_rows_shows_history_then_grid() {
        let mut buf = GridBuffer::new(2, 4);
        // Two lines of history: "AAAA", then "BBBB".
        for (col, c) in "AAAA".chars().enumerate() {
            buf.main.grid.set(
                0,
                col,
                Cell {
                    c,
                    ..Cell::default()
                },
            );
        }
        buf.main.cursor_row = 1;
        buf.main_line_feed(); // history: AAAA
        for col in 0..4 {
            buf.main.grid.set(
                0,
                col,
                Cell {
                    c: 'B',
                    ..Cell::default()
                },
            );
        }
        buf.main.cursor_row = 1;
        buf.main_line_feed(); // history: AAAA, BBBB
                              // Paint the current screen row (the grid is otherwise blank).
        for col in 0..4 {
            buf.main.grid.set(
                0,
                col,
                Cell {
                    c: 'C',
                    ..Cell::default()
                },
            );
        }

        // At the bottom the view is just the active grid.
        let bottom = buf.visible_rows();
        assert_eq!(row_text(&bottom[0]), "CCCC");

        // One line back shows the newest history line above the grid.
        buf.scroll_back(1);
        let scrolled = buf.visible_rows();
        assert_eq!(row_text(&scrolled[0]), "BBBB");
        assert_eq!(row_text(&scrolled[1]), "CCCC");

        // Two lines back shows the whole (two-line) history.
        buf.scroll_back(1);
        let top = buf.visible_rows();
        assert_eq!(row_text(&top[0]), "AAAA");
        assert_eq!(row_text(&top[1]), "BBBB");
    }

    #[test]
    fn test_scroll_back_stops_at_oldest_line() {
        let mut buf = GridBuffer::new(2, 4);
        buf.push_scrollback_line();
        buf.push_scrollback_line();
        buf.scroll_back(100);
        assert_eq!(buf.scroll_offset(), 2);
        // Scrolling further cannot go past the oldest line.
        assert_eq!(buf.visible_rows().len(), 2);
    }

    #[test]
    fn test_scroll_forward_and_to_bottom() {
        let mut buf = GridBuffer::new(2, 4);
        buf.push_scrollback_line();
        buf.push_scrollback_line();
        buf.scroll_back(2);
        buf.scroll_forward(1);
        assert_eq!(buf.scroll_offset(), 1);
        buf.scroll_to_bottom();
        assert_eq!(buf.scroll_offset(), 0);
    }

    #[test]
    fn test_scroll_is_ignored_on_alternate_screen() {
        let mut buf = GridBuffer::new(2, 4);
        buf.push_scrollback_line();
        buf.enter_alternate_screen();
        buf.scroll_back(5);
        assert_eq!(buf.scroll_offset(), 0);
        buf.scroll_to_top();
        assert_eq!(buf.scroll_offset(), 0);
    }

    /// Text of a row of cells.
    fn row_text(row: &[Cell]) -> String {
        row.iter().map(|cell| cell.c).collect()
    }

    #[test]
    fn test_scroll_down_inserts_blank_line_at_top() {
        // RF-55 / CL-17: RI at the top of the screen inserts a blank row above
        // and drops the bottom row.
        let mut buf = ScreenBuffer::new(3, 2);
        for (row, c) in ['a', 'b', 'c'].into_iter().enumerate() {
            buf.grid.set(
                row,
                0,
                Cell {
                    c,
                    ..Cell::default()
                },
            );
        }

        buf.scroll_down(1);

        assert_eq!(buf.grid.get(0, 0).c, ' ');
        assert_eq!(buf.grid.get(1, 0).c, 'a');
        assert_eq!(buf.grid.get(2, 0).c, 'b');
        // The cursor is not moved by a scroll.
        assert_eq!(buf.cursor(), (0, 0));
    }

    #[test]
    fn test_scroll_down_more_than_screen_clears() {
        let mut buf = ScreenBuffer::new(2, 2);
        buf.grid.set(
            0,
            0,
            Cell {
                c: 'a',
                ..Cell::default()
            },
        );
        buf.scroll_down(5);
        assert_eq!(buf.grid.get(0, 0).c, ' ');
        assert_eq!(buf.grid.get(1, 0).c, ' ');
    }

    /// A screen whose first column spells `A`, `B`, `C`, … row by row.
    fn filled_screen(rows: usize, cols: usize) -> ScreenBuffer {
        let mut buf = ScreenBuffer::new(rows, cols);
        for row in 0..rows {
            buf.grid.set(
                row,
                0,
                Cell {
                    c: (b'A' + row as u8) as char,
                    ..Cell::default()
                },
            );
        }
        buf
    }

    #[test]
    fn test_scroll_up_respects_region() {
        // RF-57 / CL-20: with a region, scrolling up moves only its rows.
        let mut buf = filled_screen(5, 2);
        buf.set_scroll_region(1, 3);
        buf.set_cursor(3, 0); // bottom margin
        buf.scroll_up(1);

        assert_eq!(buf.grid.get(0, 0).c, 'A'); // above the region
        assert_eq!(buf.grid.get(1, 0).c, 'C');
        assert_eq!(buf.grid.get(2, 0).c, 'D');
        assert_eq!(buf.grid.get(3, 0).c, ' '); // revealed inside the region
        assert_eq!(buf.grid.get(4, 0).c, 'E'); // below the region
    }

    #[test]
    fn test_scroll_down_respects_region() {
        let mut buf = filled_screen(5, 2);
        buf.set_scroll_region(1, 3);
        buf.scroll_down(1);

        assert_eq!(buf.grid.get(0, 0).c, 'A');
        assert_eq!(buf.grid.get(1, 0).c, ' ');
        assert_eq!(buf.grid.get(2, 0).c, 'B');
        assert_eq!(buf.grid.get(3, 0).c, 'C');
        assert_eq!(buf.grid.get(4, 0).c, 'E');
    }

    #[test]
    fn test_insert_lines_outside_region_is_ignored() {
        // RF-57: xterm discards IL/DL when the cursor is outside the margins.
        let mut buf = filled_screen(5, 2);
        buf.set_scroll_region(1, 3);
        buf.set_cursor(0, 0); // above the region
        buf.insert_lines(1);

        assert_eq!(buf.grid.get(0, 0).c, 'A');
        assert_eq!(buf.grid.get(1, 0).c, 'B');
        assert_eq!(buf.grid.get(2, 0).c, 'C');
    }

    #[test]
    fn test_insert_and_delete_lines_respect_region() {
        let mut buf = filled_screen(5, 2);
        buf.set_scroll_region(1, 3);
        buf.set_cursor(2, 0); // inside the region
        buf.insert_lines(1);
        // Region rows 1..3 (B, C, D) become B, blank, C: D is pushed out.
        assert_eq!(buf.grid.get(0, 0).c, 'A');
        assert_eq!(buf.grid.get(1, 0).c, 'B');
        assert_eq!(buf.grid.get(2, 0).c, ' ');
        assert_eq!(buf.grid.get(3, 0).c, 'C');
        assert_eq!(buf.grid.get(4, 0).c, 'E');

        let mut buf = filled_screen(5, 2);
        buf.set_scroll_region(1, 3);
        buf.set_cursor(2, 0);
        buf.delete_lines(1);
        assert_eq!(buf.grid.get(1, 0).c, 'B');
        assert_eq!(buf.grid.get(2, 0).c, 'D');
        assert_eq!(buf.grid.get(3, 0).c, ' ');
        assert_eq!(buf.grid.get(4, 0).c, 'E');
    }

    #[test]
    fn test_main_line_feed_respects_region_and_scrollback() {
        // RF-57 / CL-20: a line feed at the region's bottom margin scrolls only
        // the region and never feeds the scrollback.
        let mut buf = GridBuffer::new(5, 2);
        for row in 0..5 {
            buf.main.grid.set(
                row,
                0,
                Cell {
                    c: (b'A' + row as u8) as char,
                    ..Cell::default()
                },
            );
        }
        buf.main.set_scroll_region(1, 3);
        buf.main.set_cursor(3, 0);

        buf.main_line_feed();

        assert_eq!(buf.main.grid.get(0, 0).c, 'A');
        assert_eq!(buf.main.grid.get(1, 0).c, 'C');
        assert_eq!(buf.main.grid.get(2, 0).c, 'D');
        assert_eq!(buf.main.grid.get(3, 0).c, ' ');
        assert_eq!(buf.main.grid.get(4, 0).c, 'E');
        assert_eq!(buf.main.cursor_row, 3);
        assert_eq!(buf.scrollback_len(), 0);
    }

    #[test]
    fn test_resize_updates_full_screen_region() {
        let mut buf = ScreenBuffer::new(3, 2);
        assert_eq!((buf.scroll_top, buf.scroll_bottom), (0, 2));
        buf.resize(5, 2);
        assert_eq!((buf.scroll_top, buf.scroll_bottom), (0, 4));
    }

    #[test]
    fn test_resize_clamps_partial_region() {
        let mut buf = ScreenBuffer::new(5, 2);
        buf.set_scroll_region(1, 4);
        buf.resize(3, 2);
        assert_eq!((buf.scroll_top, buf.scroll_bottom), (1, 2));
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
