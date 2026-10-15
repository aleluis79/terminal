//! Configuration loader: reads TOML file, generates defaults.

use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::info;

use crate::config::Config;
use crate::error::ConfigError;

/// Find the configuration file path.
///
/// Linux: `$XDG_CONFIG_HOME/terminal/config.toml`, falling back to
/// `$HOME/.config/terminal/config.toml`.
/// Windows: `%APPDATA%\terminal\config.toml`.
///
/// # Panics
///
/// Panics if the environment variables that make up the config directory are
/// missing: `APPDATA` on Windows, `XDG_CONFIG_HOME`/`HOME` on Unix.
pub fn find_config_path() -> PathBuf {
    #[cfg(windows)]
    let config_dir = PathBuf::from(std::env::var("APPDATA").expect("APPDATA not set"));

    #[cfg(not(windows))]
    let config_dir = if let Ok(val) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(val)
    } else {
        let home = std::env::var("HOME").expect("HOME not set");
        PathBuf::from(home).join(".config")
    };

    config_dir.join("terminal").join("config.toml")
}

/// Load configuration from file.
///
/// If the file doesn't exist, returns the default config.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        info!("Config file not found, using defaults: {:?}", path);
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content).map_err(|e| ConfigError::ParseError {
        path: path.to_path_buf(),
        error: e.to_string(),
    })?;

    info!("Config loaded from: {:?}", path);
    Ok(config)
}

/// Generate a default configuration file.
///
/// Creates the directory structure and writes the default TOML.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot be written.
pub fn generate_default_config(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(()); // Already exists
    }

    // Create parent directories
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = Config::default();
    let toml = toml::to_string_pretty(&config)?;
    std::fs::write(path, toml)?;

    info!("Default config generated: {:?}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_config_nonexistent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let config = load_config(&path).unwrap();
        assert_eq!(config.window.title, "Terminal");
    }

    #[test]
    fn test_generate_default_config() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        generate_default_config(&path).unwrap();
        assert!(path.exists());

        let config = load_config(&path).unwrap();
        assert_eq!(config.window.title, "Terminal");
    }

    #[test]
    fn test_generate_default_config_no_overwrite() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        // Create with custom content
        std::fs::write(&path, "[window]\ntitle = \"Custom\"\n").unwrap();
        generate_default_config(&path).unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.window.title, "Custom");
    }
}
