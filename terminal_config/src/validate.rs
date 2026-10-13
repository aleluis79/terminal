//! Configuration value validation.
//!
//! Validates configuration values and returns warnings for invalid ones.
//! Invalid values are replaced with defaults.

use tracing::warn;

use crate::config::{Config, CursorStyle};

/// Validate a configuration and return a cleaned version.
///
/// Invalid values are replaced with defaults and logged as warnings.
pub fn validate(config: Config) -> Config {
    let mut config = config;

    // Validate font size
    if config.font.size <= 0.0 || config.font.size > 100.0 {
        warn!("Invalid font size: {}. Using default.", config.font.size);
        config.font.size = 14.0;
    }

    // Validate opacity
    if config.window.opacity < 0.0 || config.window.opacity > 1.0 {
        warn!("Invalid opacity: {}. Using default.", config.window.opacity);
        config.window.opacity = 1.0;
    }

    // Validate dimensions
    if config.window.dimensions.columns == 0
        || config.window.dimensions.columns > 500
        || config.window.dimensions.rows == 0
        || config.window.dimensions.rows > 200
    {
        warn!(
            "Invalid dimensions: {}x{}. Using default.",
            config.window.dimensions.columns, config.window.dimensions.rows
        );
        config.window.dimensions = crate::config::Dimensions::default();
    }

    // Validate cursor blink interval
    if config.cursor.blink_interval > 10_000 {
        warn!(
            "Invalid blink interval: {}. Using default.",
            config.cursor.blink_interval
        );
        config.cursor.blink_interval = 0;
    }

    config
}

/// Validate a single font family name.
///
/// Returns true if the font family is non-empty.
pub fn validate_font_family(family: &str) -> bool {
    !family.trim().is_empty()
}

/// Validate a cursor style string.
pub fn validate_cursor_style(style: &str) -> Option<CursorStyle> {
    match style.to_lowercase().as_str() {
        "block" => Some(CursorStyle::Block),
        "underline" => Some(CursorStyle::Underline),
        "beam" => Some(CursorStyle::Beam),
        _ => {
            warn!("Invalid cursor style: {}. Using default.", style);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FontConfig, WindowConfig};

    #[test]
    fn test_validate_font_size() {
        let mut config = Config::default();
        config.font.size = -1.0;
        let validated = validate(config);
        assert!((validated.font.size - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_font_size_valid() {
        let config = Config {
            font: crate::config::FontConfig {
                size: 16.0,
                ..FontConfig::default()
            },
            ..Config::default()
        };
        let validated = validate(config);
        assert!((validated.font.size - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_opacity() {
        let mut config = Config::default();
        config.window.opacity = 2.0;
        let validated = validate(config);
        assert!((validated.window.opacity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_opacity_valid() {
        let config = Config {
            window: crate::config::WindowConfig {
                opacity: 0.8,
                ..WindowConfig::default()
            },
            ..Config::default()
        };
        let validated = validate(config);
        assert!((validated.window.opacity - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_dimensions() {
        let mut config = Config::default();
        config.window.dimensions.columns = 0;
        let validated = validate(config);
        assert_eq!(validated.window.dimensions.columns, 80);
    }

    #[test]
    fn test_validate_dimensions_valid() {
        let config = Config {
            window: crate::config::WindowConfig {
                dimensions: crate::config::Dimensions {
                    columns: 120,
                    rows: 40,
                },
                ..WindowConfig::default()
            },
            ..Config::default()
        };
        let validated = validate(config);
        assert_eq!(validated.window.dimensions.columns, 120);
        assert_eq!(validated.window.dimensions.rows, 40);
    }

    #[test]
    fn test_validate_cursor_style_block() {
        assert_eq!(validate_cursor_style("block"), Some(CursorStyle::Block));
    }

    #[test]
    fn test_validate_cursor_style_underline() {
        assert_eq!(
            validate_cursor_style("underline"),
            Some(CursorStyle::Underline)
        );
    }

    #[test]
    fn test_validate_cursor_style_beam() {
        assert_eq!(validate_cursor_style("beam"), Some(CursorStyle::Beam));
    }

    #[test]
    fn test_validate_cursor_style_invalid() {
        assert_eq!(validate_cursor_style("invalid"), None);
    }

    #[test]
    fn test_validate_font_family_empty() {
        assert!(!validate_font_family(""));
    }

    #[test]
    fn test_validate_font_family_valid() {
        assert!(validate_font_family("Fira Code"));
    }
}
