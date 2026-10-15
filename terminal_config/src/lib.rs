//! Configuration system: TOML loading, CLI parsing, validation.
//!
//! This crate handles:
//! - Loading configuration from `config.toml`
//! - Generating default configuration
//! - CLI argument parsing with override semantics
//! - Validation of configuration values

#![warn(missing_docs)]

/// Core configuration structures.
pub mod config;

/// TOML file loader.
pub mod loader;

/// CLI argument parser.
pub mod cli;

/// Shell profile resolution.
pub mod shell;

/// Configuration error types.
pub mod error;

/// Configuration value validation.
pub mod validate;
