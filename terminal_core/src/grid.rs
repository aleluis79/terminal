//! 2D grid data structure for the terminal screen.

use crate::cell::Cell;

/// A 2D grid of cells representing the terminal screen.
///
/// The grid is stored as a flat array for cache locality.
/// Access is `grid[row * cols + col]`.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Number of rows.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// Flat array of cells, row-major order.
    pub cells: Vec<Cell>,
}

impl Grid {
    /// Create a new grid with the given dimensions, filled with default cells.
    pub fn new(rows: usize, cols: usize) -> Self {
        let cells = vec![Cell::default(); rows * cols];
        Self { rows, cols, cells }
    }

    /// Get the cell at (row, col).
    pub fn get(&self, row: usize, col: usize) -> Cell {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col]
        } else {
            Cell::default()
        }
    }

    /// Set the cell at (row, col).
    pub fn set(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = cell;
        }
    }

    /// Clear the entire grid with default cells.
    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    /// Get a reference to the cell at (row, col).
    pub fn get_ref(&self, row: usize, col: usize) -> &Cell {
        if row < self.rows && col < self.cols {
            &self.cells[row * self.cols + col]
        } else {
            static DEFAULT: std::sync::LazyLock<Cell> = std::sync::LazyLock::new(Cell::default);
            &DEFAULT
        }
    }

    /// Get the number of rows.
    pub fn len(&self) -> usize {
        self.rows
    }

    /// Check if the grid is empty.
    pub fn is_empty(&self) -> bool {
        self.rows == 0 || self.cols == 0
    }

    /// Whether the cell at (row, col) is the right half (continuation) of a
    /// wide character.
    pub fn is_continuation(&self, row: usize, col: usize) -> bool {
        self.get_ref(row, col).width == 0
    }

    /// Write a wide character at (row, col), marking `col + 1` as its
    /// continuation.
    ///
    /// Any pre-existing wide character overlapping either cell is cleared
    /// first, so a wide glyph never leaves an orphaned half behind. A wide
    /// character that would not fit in the last column is written as a narrow
    /// cell instead (the caller is expected to wrap beforehand).
    pub fn set_wide(&mut self, row: usize, col: usize, cell: Cell) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        self.clear_cell(row, col);
        if col + 1 >= self.cols {
            let mut narrow = cell;
            narrow.width = 1;
            self.set(row, col, narrow);
            return;
        }
        self.clear_cell(row, col + 1);

        let header = Cell { width: 2, ..cell };
        let mut tail = cell;
        tail.c = ' ';
        tail.width = 0;
        self.set(row, col, header);
        self.set(row, col + 1, tail);
    }

    /// Clear the cell at (row, col), also clearing the other half when the
    /// cell belongs to a wide character.
    pub fn clear_cell(&mut self, row: usize, col: usize) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        let width = self.cells[row * self.cols + col].width;
        self.cells[row * self.cols + col] = Cell::default();
        match width {
            // The cleared cell was the left half: clear its continuation.
            2 if col + 1 < self.cols => {
                self.cells[row * self.cols + col + 1] = Cell::default();
            }
            // The cleared cell was the right half: clear its header.
            0 if col > 0 => {
                self.cells[row * self.cols + col - 1] = Cell::default();
            }
            _ => {}
        }
    }

    /// Clear any wide-character half left without its partner (RF-45, CL-4).
    ///
    /// After an operation that can truncate a row (resize, reflow) a wide glyph
    /// may keep only one of its two cells. Such orphans are blanked so the grid
    /// always holds whole pairs.
    pub fn sanitize_wide(&mut self) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let idx = row * self.cols + col;
                match self.cells[idx].width {
                    2 if col + 1 >= self.cols || self.cells[idx + 1].width != 0 => {
                        self.cells[idx] = Cell::default();
                    }
                    0 if col == 0 || self.cells[idx - 1].width != 2 => {
                        self.cells[idx] = Cell::default();
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Cell, CellStyle, Color};

    #[test]
    fn test_create_grid() {
        let grid = Grid::new(24, 80);
        assert_eq!(grid.len(), 24);
        assert_eq!(grid.cols, 80);
        assert_eq!(grid.get(0, 0), Cell::default());
    }

    #[test]
    fn test_set_and_get() {
        let mut grid = Grid::new(10, 10);
        let cell = Cell {
            c: 'A',
            style: CellStyle {
                fg: Color::Named(1),
                ..CellStyle::default()
            },
            width: 1,
        };
        grid.set(5, 5, cell);
        assert_eq!(grid.get(5, 5).c, 'A');
        assert_eq!(grid.get(5, 5).style.fg, Color::Named(1));
    }

    #[test]
    fn test_clear() {
        let mut grid = Grid::new(10, 10);
        grid.set(
            0,
            0,
            Cell {
                c: 'X',
                ..Cell::default()
            },
        );
        grid.clear();
        assert_eq!(grid.get(0, 0).c, ' ');
    }

    #[test]
    fn test_out_of_bounds() {
        let grid = Grid::new(24, 80);
        assert_eq!(grid.get(100, 100), Cell::default());
        assert_eq!(grid.get(24, 0), Cell::default());
        assert_eq!(grid.get(0, 80), Cell::default());
    }

    #[test]
    fn test_corner_cells() {
        let mut grid = Grid::new(5, 5);
        grid.set(
            0,
            0,
            Cell {
                c: 'T',
                ..Cell::default()
            },
        );
        grid.set(
            4,
            4,
            Cell {
                c: 'B',
                ..Cell::default()
            },
        );
        assert_eq!(grid.get(0, 0).c, 'T');
        assert_eq!(grid.get(4, 4).c, 'B');
    }

    #[test]
    fn test_center_cell() {
        let mut grid = Grid::new(10, 10);
        grid.set(
            5,
            5,
            Cell {
                c: 'C',
                ..Cell::default()
            },
        );
        assert_eq!(grid.get(5, 5).c, 'C');
    }

    #[test]
    fn test_resize_preserves_content() {
        let mut grid = Grid::new(10, 10);
        for row in 0..10 {
            for col in 0..10 {
                grid.set(
                    row,
                    col,
                    Cell {
                        c: 'A',
                        ..Cell::default()
                    },
                );
            }
        }
        // Resize to larger
        let mut new_grid = Grid::new(20, 20);
        for row in 0..10 {
            for col in 0..10 {
                new_grid.set(row, col, grid.get(row, col));
            }
        }
        assert_eq!(new_grid.get(5, 5).c, 'A');
        assert_eq!(new_grid.get(15, 15).c, ' '); // new area is empty
    }

    #[test]
    fn test_set_wide_marks_continuation() {
        let mut grid = Grid::new(2, 5);
        grid.set_wide(
            0,
            1,
            Cell {
                c: '日',
                ..Cell::default()
            },
        );

        assert_eq!(grid.get(0, 1).c, '日');
        assert_eq!(grid.get(0, 1).width, 2);
        assert!(grid.is_continuation(0, 2));
        assert_eq!(grid.get(0, 2).width, 0);
    }

    #[test]
    fn test_clear_cell_clears_both_halves() {
        let mut grid = Grid::new(2, 5);
        grid.set_wide(
            0,
            1,
            Cell {
                c: '本',
                ..Cell::default()
            },
        );

        // Clearing either half must clear the other one too.
        grid.clear_cell(0, 2);
        assert_eq!(grid.get(0, 1).c, ' ');
        assert_eq!(grid.get(0, 2).c, ' ');
        assert_eq!(grid.get(0, 1).width, 1);
        assert_eq!(grid.get(0, 2).width, 1);

        grid.set_wide(
            0,
            1,
            Cell {
                c: '本',
                ..Cell::default()
            },
        );
        grid.clear_cell(0, 1);
        assert_eq!(grid.get(0, 2).c, ' ');
    }

    #[test]
    fn test_set_wide_over_narrow_clears_previous_pairs() {
        let mut grid = Grid::new(2, 5);
        grid.set(
            0,
            0,
            Cell {
                c: 'A',
                ..Cell::default()
            },
        );
        // Overwrite starting on the neighbour of a previous wide glyph.
        grid.set_wide(
            0,
            0,
            Cell {
                c: '日',
                ..Cell::default()
            },
        );
        assert_eq!(grid.get(0, 0).c, '日');
        assert!(grid.is_continuation(0, 1));
        // The old narrow cell at column 1 is gone.
        assert_ne!(grid.get(0, 1).c, 'A');
    }

    #[test]
    fn test_set_wide_in_last_column_stays_narrow() {
        let mut grid = Grid::new(1, 4);
        grid.set_wide(
            0,
            3,
            Cell {
                c: '日',
                ..Cell::default()
            },
        );
        assert_eq!(grid.get(0, 3).c, '日');
        assert_eq!(grid.get(0, 3).width, 1);
    }
}
