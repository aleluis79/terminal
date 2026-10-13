//! A single cell in the terminal grid.

/// Foreground or background color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// One of the 16 ANSI colors.
    Named(u8),
    /// 256-color palette.
    Palette(u8),
    /// 24-bit RGB.
    Rgb {
        /// Red component (0-255).
        r: u8,
        /// Green component (0-255).
        g: u8,
        /// Blue component (0-255).
        b: u8,
    },
}

impl Default for Color {
    fn default() -> Self {
        Self::Named(7)
    }
}

impl Color {
    /// Convert to RGBA components.
    pub fn to_rgba(self, palette: &[u32; 256]) -> u32 {
        match self {
            Color::Named(n) | Color::Palette(n) => palette[n as usize],
            Color::Rgb { r, g, b } => u32::from(b) | (u32::from(g) << 8) | (u32::from(r) << 16),
        }
    }

    /// Get the color index as u8 for ANSI/palette colors, or 0 for RGB.
    ///
    /// Returns the index for `Named` and `Palette` variants,
    /// or `0` (black) for `Rgb` variants (the renderer will use the palette).
    pub fn as_u8_or_ansi(self) -> u8 {
        match self {
            Color::Named(n) | Color::Palette(n) => n,
            Color::Rgb { .. } => 0,
        }
    }
}

/// Text attributes for a cell.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
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

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            // Default foreground is ANSI white (7); default background is ANSI
            // black (0), matching `TerminalState` defaults. A background index of
            // 0 is treated by the renderer as "use the terminal background".
            fg: Color::Named(7),
            bg: Color::Named(0),
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            hidden: false,
            strikethrough: false,
        }
    }
}

/// A single character cell in the terminal grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The character to display.
    pub c: char,
    /// Text style (colors, attributes).
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            style: CellStyle::default(),
        }
    }
}
