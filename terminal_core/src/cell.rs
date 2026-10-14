//! A single cell in the terminal grid.

use unicode_width::UnicodeWidthChar;

/// Display width of a character, in terminal cells.
///
/// Returns `2` for East Asian wide and fullwidth characters (CJK, wide emoji),
/// `1` for ordinary characters, and `0` for combining marks, zero-width
/// joiners and control characters. Control characters have no width of their
/// own, so they map to `0` rather than being reported as missing.
pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

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
    /// Display width of this cell: `2` for the left half of a wide character,
    /// `1` for an ordinary character, `0` for the right half (continuation) of
    /// a wide character.
    pub width: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            style: CellStyle::default(),
            width: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::char_width;

    #[test]
    fn test_narrow_ascii() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('Z'), 1);
        assert_eq!(char_width(' '), 1);
    }

    #[test]
    fn test_wide_cjk() {
        assert_eq!(char_width('日'), 2);
        assert_eq!(char_width('本'), 2);
    }

    #[test]
    fn test_wide_emoji() {
        assert_eq!(char_width('🦀'), 2);
    }

    #[test]
    fn test_zero_width_combining_and_joiners() {
        // U+0301 COMBINING ACUTE ACCENT.
        assert_eq!(char_width('\u{0301}'), 0);
        // U+200D ZERO WIDTH JOINER.
        assert_eq!(char_width('\u{200D}'), 0);
    }

    #[test]
    fn test_zero_width_control() {
        assert_eq!(char_width('\0'), 0);
        assert_eq!(char_width('\u{07}'), 0);
    }

    #[test]
    fn test_precomposed_accent_is_narrow() {
        assert_eq!(char_width('á'), 1);
    }
}
