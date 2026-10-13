//! Color handling for the terminal renderer.
//!
//! This module handles:
//! - 16 ANSI color palette
//! - 256-color extended palette
//! - 24-bit RGB colors
//! - Color conversion to GPU format

/// Terminal color as RGBA (8-bit per channel).
pub type Color = u32;

/// Standard 16 ANSI color palette.
///
/// Colors 0-7 are the standard ANSI colors.
/// Colors 8-15 are the bright/bold variants.
#[derive(Debug, Clone, Copy)]
pub struct TerminalPalette {
    /// Standard colors (0-7).
    pub standard: [Color; 8],
    /// Bright colors (8-15).
    pub bright: [Color; 8],
    /// Default foreground color (index 7).
    pub foreground: Color,
    /// Default background color (index 0).
    pub background: Color,
}

impl Default for TerminalPalette {
    fn default() -> Self {
        Self {
            standard: [
                0x0000_00FF, // 0: Black
                0xCC04_00FF, // 1: Red (Dracula-like)
                0x4E9A_06FF, // 2: Green
                0xC0A3_60FF, // 3: Yellow
                0x3465_94FF, // 4: Blue
                0x7550_7FFF, // 5: Magenta
                0x0698_9AFF, // 6: Cyan
                0xD3D7_CFFF, // 7: White
            ],
            bright: [
                0x5557_53FF, // 8: Bright Black (Gray)
                0xEF29_29FF, // 9: Bright Red
                0x8AE2_34FF, // 10: Bright Green
                0xFCE9_4FFF, // 11: Bright Yellow
                0x729F_CFFF, // 12: Bright Blue
                0xAD7F_A8FF, // 13: Bright Magenta
                0x34E2_E2FF, // 14: Bright Cyan
                0xFFFF_FF99, // 15: Bright White
            ],
            foreground: 0xD3D7_CFFF, // White
            background: 0x281B_2AFF, // Dark purple (Dracula-like)
        }
    }
}

impl TerminalPalette {
    /// Get a color by index (0-255).
    ///
    /// # Arguments
    ///
    /// * `index` - Color index (0-15 for ANSI, 16-255 for extended).
    pub fn get(&self, index: u8) -> Color {
        match index {
            0..=7 => self.standard[index as usize],
            8..=15 => self.bright[index as usize - 8],
            _ => 0x8080_80FF, // Extended colors: return gray as fallback
        }
    }

    /// Convert the palette to a 256-element array for quick lookup.
    pub fn as_array(&self) -> [Color; 256] {
        let mut arr = [0u32; 256];

        // Standard colors
        arr[..8].copy_from_slice(&self.standard);
        // Bright colors
        arr[8..16].copy_from_slice(&self.bright);

        // 6x6x6 color cube (16-231) = 216 colors
        // Values: 0x00, 0x5F, 0x87, 0xAF, 0xD7, 0xFF
        // Format: (r << 16) | (g << 8) | b (no alpha, consistent with standard colors)
        let cube_values: [u8; 6] = [0x00, 0x5F, 0x87, 0xAF, 0xD7, 0xFF];
        let mut idx = 16;
        for &r in &cube_values {
            for &g in &cube_values {
                for &b in &cube_values {
                    if idx < 232 {
                        arr[idx] = u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
                        idx += 1;
                    }
                }
            }
        }

        // Grayscale ramp (232-255)
        // Format: (a << 24) | (r << 16) | (g << 8) | b
        for i in 0..24 {
            let gray_idx = 232 + i;
            let gray: u8 = 0x08 + (i * 0x0A) as u8;
            arr[gray_idx] =
                0xFF00_0000 | u32::from(gray) << 16 | u32::from(gray) << 8 | u32::from(gray);
        }

        arr
    }

    /// Set a custom color in the palette.
    pub fn set_color(&mut self, index: u8, color: Color) {
        match index {
            0..=7 => self.standard[index as usize] = color,
            8..=15 => self.bright[index as usize - 8] = color,
            _ => {} // Extended colors not directly settable
        }
    }
}

/// Convert a color value to RGBA components.
///
/// # Arguments
///
/// * `color` - Color as u32 (RGBA, 8-bit per channel).
/// * `palette` - Optional palette for indexed colors.
pub fn to_rgba(color: Color, _palette: &[u32; 256]) -> u32 {
    // Color is already in RGBA format (ABGR in memory)
    color
}

/// Parse an RGB color from components.
///
/// # Arguments
///
/// * `r` - Red (0-255).
/// * `g` - Green (0-255).
/// * `b` - Blue (0-255).
/// * `a` - Alpha (0-255, default 255).
pub fn rgb(r: u8, g: u8, b: u8, a: u8) -> Color {
    u32::from(a) << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

/// Parse an RGB color with full alpha.
pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    rgb(r, g, b, a)
}

/// Pack channels into an RGBA-ordered color (`r << 24 | g << 16 | b << 8 | a`).
///
/// This is the ordering used by [`TerminalPalette`] entries and by the
/// renderer when unpacking colors.
pub fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    u32::from(r) << 24 | u32::from(g) << 16 | u32::from(b) << 8 | u32::from(a)
}

/// Unpack an RGBA-ordered color into normalized `[r, g, b, a]` (0.0..=1.0).
pub fn unpack_rgba(color: Color) -> [f32; 4] {
    let r = ((color >> 24) & 0xFF) as u8;
    let g = ((color >> 16) & 0xFF) as u8;
    let b = ((color >> 8) & 0xFF) as u8;
    let a = (color & 0xFF) as u8;
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ]
}

/// Parse a hex color string (e.g., "#FF0000" or "FF0000").
///
/// # Arguments
///
/// * `hex` - Hex color string.
pub fn parse_hex_color(hex: &str) -> Option<Color> {
    let cleaned = hex.trim().trim_start_matches('#');
    if cleaned.len() != 6 && cleaned.len() != 8 {
        return None;
    }

    let r = u8::from_str_radix(&cleaned[0..2], 16).ok()?;
    let g = u8::from_str_radix(&cleaned[2..4], 16).ok()?;
    let b = u8::from_str_radix(&cleaned[4..6], 16).ok()?;
    let a = if cleaned.len() == 8 {
        u8::from_str_radix(&cleaned[6..8], 16).ok()?
    } else {
        255
    };

    Some(rgba(r, g, b, a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_palette_default() {
        let palette = TerminalPalette::default();
        assert_eq!(palette.get(0), 0x0000_00FF); // Black
        assert_eq!(palette.get(7), 0xD3D7_CFFF); // White
        assert_eq!(palette.get(15), 0xFFFF_FF99); // Bright White
    }

    #[test]
    fn test_palette_array() {
        let palette = TerminalPalette::default();
        let arr = palette.as_array();
        assert_eq!(arr.len(), 256);
        assert_eq!(arr[0], 0x0000_00FF); // Black
        assert_eq!(arr[15], 0xFFFF_FF99); // Bright White
        assert_eq!(arr[16], 0x0000_0000); // Start of color cube (black)
                                          // Grayscale: alpha in high byte, RGB in low 3 bytes
        assert_eq!(arr[232], 0xFF08_0808); // Start of grayscale (gray=0x08)
                                           // End of grayscale: gray = 0x08 + 23*10 = 0xEE
        assert_eq!(arr[255], 0xFFEE_EEEE);
    }

    #[test]
    fn test_rgb() {
        // rgb(r, g, b, a) -> (a << 24) | (r << 16) | (g << 8) | b
        assert_eq!(rgb(255, 0, 0, 255), 0xFFFF_0000); // ARGB: 255,255,0,0
        assert_eq!(rgb(0, 255, 0, 255), 0xFF00_FF00); // ARGB: 255,0,255,0
        assert_eq!(rgb(0, 0, 255, 255), 0xFF00_00FF); // ARGB: 255,0,0,255
    }

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#FF0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_hex_color("00FF00"), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_hex_color("#0000FF00"), Some(rgba(0, 0, 255, 0)));
        assert!(parse_hex_color("GGG").is_none());
        assert!(parse_hex_color("").is_none());
    }

    #[test]
    fn test_to_rgba() {
        let palette = TerminalPalette::default();
        let arr = palette.as_array();
        assert_eq!(to_rgba(0xFF00_00FF, &arr), 0xFF00_00FF);
    }

    #[test]
    fn test_pack_unpack_rgba() {
        let color = pack_rgba(0x12, 0x34, 0x56, 0xFF);
        assert_eq!(color, 0x1234_56FF);
        let [r, g, b, a] = unpack_rgba(color);
        assert!((r - f32::from(0x12_u8) / 255.0).abs() < f32::EPSILON);
        assert!((g - f32::from(0x34_u8) / 255.0).abs() < f32::EPSILON);
        assert!((b - f32::from(0x56_u8) / 255.0).abs() < f32::EPSILON);
        assert!((a - 1.0).abs() < f32::EPSILON);
    }
}
