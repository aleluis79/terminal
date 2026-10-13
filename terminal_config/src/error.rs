//! Configuration error types.

use thiserror::Error;

/// Errors that can occur during configuration loading.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Error reading the configuration file.
    #[error("Failed to read config file: {path}")]
    ReadError {
        /// Path to the config file.
        path: std::path::PathBuf,
        /// The underlying IO error.
        #[source]
        error: std::io::Error,
    },

    /// Error parsing the configuration file.
    #[error("Failed to parse config file: {path}")]
    ParseError {
        /// Path to the config file.
        path: std::path::PathBuf,
        /// The underlying parse error.
        error: String,
    },

    /// Error serializing configuration.
    #[error("Failed to serialize config")]
    SerializeError(#[from] toml::ser::Error),

    /// Invalid configuration value.
    #[error("Invalid config value for {field}: {message}")]
    InvalidValue {
        /// Configuration field name.
        field: String,
        /// Description of the invalid value.
        message: String,
    },
}
