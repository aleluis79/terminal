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
}
