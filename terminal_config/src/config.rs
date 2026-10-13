//! Configuration structures.

use serde::{Deserialize, Serialize};

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Config {
    /// Window settings.
    #[serde(default)]
    pub window: WindowConfig,
    /// Font settings.
    #[serde(default)]
    pub font: FontConfig,
    /// Cursor settings.
    #[serde(default)]
    pub cursor: CursorConfig,
    /// Color scheme.
    #[serde(default)]
    pub colors: ColorScheme,
    /// Keyboard bindings.
    #[serde(default)]
    pub keyboard: KeyboardConfig,
}

/// Window configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowConfig {
    /// Window title.
    #[serde(default = "default_title")]
    pub title: String,
    /// Window dimensions in cells.
    #[serde(default)]
    pub dimensions: Dimensions,
    /// Window opacity (0.0 = transparent, 1.0 = opaque).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: default_title(),
            dimensions: Dimensions::default(),
            opacity: default_opacity(),
        }
    }
}

/// Window dimensions in cells.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dimensions {
    /// Number of columns.
    #[serde(default = "default_columns")]
    pub columns: u16,
    /// Number of rows.
    #[serde(default = "default_rows")]
    pub rows: u16,
}

impl Default for Dimensions {
    fn default() -> Self {
        Self {
            columns: default_columns(),
            rows: default_rows(),
        }
    }
}

/// Font configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontConfig {
    /// Font family name.
    #[serde(default = "default_font_family")]
    pub family: String,
    /// Font size in points.
    #[serde(default = "default_font_size")]
    pub size: f32,
    /// Enable ligatures.
    #[serde(default = "default_ligatures")]
    pub ligatures: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: default_font_family(),
            size: default_font_size(),
            ligatures: default_ligatures(),
        }
    }
}

/// Cursor configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CursorConfig {
    /// Cursor style.
    #[serde(default)]
    pub style: CursorStyle,
    /// Cursor blink interval in milliseconds (0 = disabled).
    #[serde(default)]
    pub blink_interval: u64,
}

/// Cursor style.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CursorStyle {
    /// Block cursor.
    #[default]
    Block,
    /// Underline cursor.
    Underline,
    /// Beam (vertical bar) cursor.
    Beam,
}

/// Color scheme.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ColorScheme {
    /// Primary colors (foreground, background, cursor).
    #[serde(default)]
    pub primary: PrimaryColors,
    /// ANSI colors (0-15).
    #[serde(default)]
    pub ansi: AnsiColors,
    /// Extended colors (16-255).
    #[serde(default)]
    pub extended: ExtendedColors,
}

/// Primary colors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrimaryColors {
    /// Foreground color (hex).
    #[serde(default = "default_foreground")]
    pub foreground: String,
    /// Background color (hex).
    #[serde(default = "default_background")]
    pub background: String,
    /// Cursor color.
    #[serde(default = "default_cursor_color")]
    pub cursor: String,
}

impl Default for PrimaryColors {
    fn default() -> Self {
        Self {
            foreground: default_foreground(),
            background: default_background(),
            cursor: default_cursor_color(),
        }
    }
}

/// ANSI colors (0-15).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnsiColors {
    /// Color 0: Black.
    #[serde(default = "default_color_0")]
    pub black: String,
    /// Color 1: Red.
    #[serde(default = "default_color_1")]
    pub red: String,
    /// Color 2: Green.
    #[serde(default = "default_color_2")]
    pub green: String,
    /// Color 3: Yellow.
    #[serde(default = "default_color_3")]
    pub yellow: String,
    /// Color 4: Blue.
    #[serde(default = "default_color_4")]
    pub blue: String,
    /// Color 5: Magenta.
    #[serde(default = "default_color_5")]
    pub magenta: String,
    /// Color 6: Cyan.
    #[serde(default = "default_color_6")]
    pub cyan: String,
    /// Color 7: White.
    #[serde(default = "default_color_7")]
    pub white: String,
    /// Color 8: Bright Black.
    #[serde(default = "default_color_8")]
    pub bright_black: String,
    /// Color 9: Bright Red.
    #[serde(default = "default_color_9")]
    pub bright_red: String,
    /// Color 10: Bright Green.
    #[serde(default = "default_color_10")]
    pub bright_green: String,
    /// Color 11: Bright Yellow.
    #[serde(default = "default_color_11")]
    pub bright_yellow: String,
    /// Color 12: Bright Blue.
    #[serde(default = "default_color_12")]
    pub bright_blue: String,
    /// Color 13: Bright Magenta.
    #[serde(default = "default_color_13")]
    pub bright_magenta: String,
    /// Color 14: Bright Cyan.
    #[serde(default = "default_color_14")]
    pub bright_cyan: String,
    /// Color 15: Bright White.
    #[serde(default = "default_color_15")]
    pub bright_white: String,
}

impl Default for AnsiColors {
    fn default() -> Self {
        Self {
            black: default_color_0(),
            red: default_color_1(),
            green: default_color_2(),
            yellow: default_color_3(),
            blue: default_color_4(),
            magenta: default_color_5(),
            cyan: default_color_6(),
            white: default_color_7(),
            bright_black: default_color_8(),
            bright_red: default_color_9(),
            bright_green: default_color_10(),
            bright_yellow: default_color_11(),
            bright_blue: default_color_12(),
            bright_magenta: default_color_13(),
            bright_cyan: default_color_14(),
            bright_white: default_color_15(),
        }
    }
}

/// Extended colors (16-255).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ExtendedColors {
    /// Custom colors 16-231 (6×6×6 color cube).
    #[serde(default)]
    pub cube: Vec<String>,
    /// Grayscale ramp 232-255.
    #[serde(default)]
    pub grayscale: Vec<String>,
}

/// Keyboard bindings configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KeyboardConfig {
    /// Custom key bindings.
    #[serde(default)]
    pub bindings: Vec<KeyBinding>,
}

/// A single keyboard binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyBinding {
    /// Key combination (e.g., "Ctrl+Shift+N").
    pub key: String,
    /// Action to perform (e.g., `"NewTab"`).
    pub action: String,
}

// Default value functions
fn default_title() -> String {
    "Terminal".to_string()
}

fn default_columns() -> u16 {
    80
}

fn default_rows() -> u16 {
    24
}

fn default_opacity() -> f32 {
    1.0
}

fn default_font_family() -> String {
    "JetBrains Mono".to_string()
}

fn default_font_size() -> f32 {
    14.0
}

fn default_ligatures() -> bool {
    true
}

fn default_foreground() -> String {
    "#ffffff".to_string()
}

fn default_background() -> String {
    "#1e1e2e".to_string()
}

fn default_cursor_color() -> String {
    "#ffffff".to_string()
}

fn default_color_0() -> String {
    "#000000".to_string()
}

fn default_color_1() -> String {
    "#cc0000".to_string()
}

fn default_color_2() -> String {
    "#4e9a06".to_string()
}

fn default_color_3() -> String {
    "#c4a000".to_string()
}

fn default_color_4() -> String {
    "#3465a4".to_string()
}

fn default_color_5() -> String {
    "#75507b".to_string()
}

fn default_color_6() -> String {
    "#06989a".to_string()
}

fn default_color_7() -> String {
    "#d3d7cf".to_string()
}

fn default_color_8() -> String {
    "#555753".to_string()
}

fn default_color_9() -> String {
    "#ef2929".to_string()
}

fn default_color_10() -> String {
    "#8ae234".to_string()
}

fn default_color_11() -> String {
    "#fce94f".to_string()
}

fn default_color_12() -> String {
    "#729fcf".to_string()
}

fn default_color_13() -> String {
    "#ad7fa8".to_string()
}

fn default_color_14() -> String {
    "#34e2e2".to_string()
}

fn default_color_15() -> String {
    "#eeeeec".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.window.title, "Terminal");
        assert_eq!(config.window.dimensions.columns, 80);
        assert_eq!(config.window.dimensions.rows, 24);
        assert!((config.window.opacity - 1.0).abs() < f32::EPSILON);
        assert!((config.font.size - 14.0).abs() < f32::EPSILON);
        assert!(config.font.ligatures);
    }

    #[test]
    fn test_config_serde() {
        let config = Config::default();
        let toml = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_custom_config() {
        let toml = r#"
[window]
title = "My Terminal"
opacity = 0.9
[window.dimensions]
columns = 120
rows = 40

[font]
family = "Fira Code"
size = 16.0
ligatures = false

[cursor]
style = "Beam"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.window.title, "My Terminal");
        assert_eq!(config.window.dimensions.columns, 120);
        assert_eq!(config.window.dimensions.rows, 40);
        assert!((config.window.opacity - 0.9).abs() < f32::EPSILON);
        assert_eq!(config.font.family, "Fira Code");
        assert!((config.font.size - 16.0).abs() < f32::EPSILON);
        assert!(!config.font.ligatures);
    }
}
