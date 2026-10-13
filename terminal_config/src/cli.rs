//! CLI argument parser.

use anyhow::Result;
use clap::Parser;

use crate::config::Config;

/// Terminal emulator command line arguments.
#[derive(Debug, Parser)]
#[command(name = "terminal", about = "A modern terminal emulator")]
pub struct Cli {
    /// Path to configuration file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<std::path::PathBuf>,

    /// Font family name.
    #[arg(long, value_name = "FAMILY")]
    pub font: Option<String>,

    /// Font size in points.
    #[arg(long, value_name = "SIZE")]
    pub font_size: Option<f32>,

    /// Window dimensions (`WxH`, e.g., "80x24").
    #[arg(long, value_name = "WxH")]
    pub geometry: Option<String>,

    /// Window opacity (0.0-1.0).
    #[arg(long, value_name = "OPACITY")]
    pub opacity: Option<f32>,
}

/// Merge CLI arguments into config (CLI overrides config file).
pub fn merge_cli(config: Config, cli: &Cli) -> Config {
    let mut config = config;

    if let Some(font) = &cli.font {
        config.font.family.clone_from(font);
    }
    if let Some(size) = cli.font_size {
        config.font.size = size;
    }
    if let Some(geometry) = &cli.geometry {
        if let Ok((cols, rows)) = parse_geometry(geometry) {
            config.window.dimensions.columns = cols;
            config.window.dimensions.rows = rows;
        }
    }
    if let Some(opacity) = cli.opacity {
        config.window.opacity = opacity;
    }

    config
}

/// Parse geometry string `WxH` into (columns, rows).
fn parse_geometry(geometry: &str) -> Result<(u16, u16)> {
    let parts: Vec<&str> = geometry.split('x').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid geometry format: {geometry}. Expected WxH");
    }
    let cols = parts[0].parse::<u16>()?;
    let rows = parts[1].parse::<u16>()?;
    Ok((cols, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_geometry() {
        assert_eq!(parse_geometry("80x24").unwrap(), (80, 24));
        assert_eq!(parse_geometry("120x40").unwrap(), (120, 40));
        assert!(parse_geometry("invalid").is_err());
        assert!(parse_geometry("80x").is_err());
        assert!(parse_geometry("x24").is_err());
    }

    #[test]
    fn test_merge_cli_no_overrides() {
        let config = Config::default();
        let cli = Cli::parse_from(["terminal"]);
        let merged = merge_cli(config, &cli);
        assert_eq!(merged, Config::default());
    }

    #[test]
    fn test_merge_cli_font_override() {
        let config = Config::default();
        let cli = Cli::parse_from(["terminal", "--font", "Fira Code"]);
        let merged = merge_cli(config, &cli);
        assert_eq!(merged.font.family, "Fira Code");
    }

    #[test]
    fn test_merge_cli_font_size_override() {
        let config = Config::default();
        let cli = Cli::parse_from(["terminal", "--font-size", "16.0"]);
        let merged = merge_cli(config, &cli);
        assert!((merged.font.size - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_merge_cli_geometry_override() {
        let config = Config::default();
        let cli = Cli::parse_from(["terminal", "--geometry", "120x40"]);
        let merged = merge_cli(config, &cli);
        assert_eq!(merged.window.dimensions.columns, 120);
        assert_eq!(merged.window.dimensions.rows, 40);
    }

    #[test]
    fn test_merge_cli_opacity_override() {
        let config = Config::default();
        let cli = Cli::parse_from(["terminal", "--opacity", "0.8"]);
        let merged = merge_cli(config, &cli);
        assert!((merged.window.opacity - 0.8).abs() < f32::EPSILON);
    }
}
